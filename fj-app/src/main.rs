mod args;
mod camera;
mod config;
mod graphics;
mod input;
mod mesh;
mod window;

use std::path::PathBuf;
use std::{collections::HashMap, time::Instant};

use fj_debug::DebugInfo;
use fj_host::Model;
use fj_kernel::algorithms::triangulate;
use fj_math::{Aabb, Scalar, Triangle};
use fj_operations::ToShape as _;
use futures::executor::block_on;
use tracing::trace;
use tracing_subscriber::fmt::format;
use tracing_subscriber::EnvFilter;
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
};

use crate::{
    args::Args,
    camera::Camera,
    config::Config,
    graphics::{DrawConfig, Renderer},
    mesh::MeshMaker,
    window::Window,
};

fn main() -> anyhow::Result<()> {
    // Respect `RUST_LOG`. If that's not defined or erroneous, log warnings and
    // above.
    //
    // It would be better to fail, if `RUST_LOG` is erroneous, but I don't know
    // how to distinguish between that and the "not defined" case.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("WARN")),
        )
        .event_format(format().pretty())
        .init();

    let args = Args::parse();
    let config = Config::load()?;

    let mut path = config.default_path.unwrap_or_else(|| PathBuf::from(""));
    match args.model.or(config.default_model) {
        Some(model) => {
            path.push(model);
        }
        None => {
            anyhow::bail!(
                "No model specified, and no default model configured.\n\
                Specify a model by passing `--model path/to/model`."
            );
        }
    }

    let model = Model::from_path(path, config.target_dir)?;

    let mut parameters = HashMap::new();
    for parameter in args.parameters {
        let mut parameter = parameter.splitn(2, '=');

        let key = parameter
            .next()
            .expect("model parameter: key not found")
            .to_owned();
        let value = parameter
            .next()
            .expect("model parameter: value not found")
            .to_owned();

        parameters.insert(key, value);
    }

    let shape_processor = ShapeProcessor::new(args.tolerance)?;

    if let Some(path) = args.export {
        let shape = model.load_once(&parameters)?;
        let shape = shape_processor.process(&shape);

        let mut mesh_maker = MeshMaker::new();

        for triangle in shape.triangles {
            for vertex in triangle.points() {
                mesh_maker.push(vertex);
            }
        }

        let vertices =
            mesh_maker.vertices().map(|vertex| vertex.into()).collect();

        let indices: Vec<_> = mesh_maker.indices().collect();
        let triangles = indices
            .chunks(3)
            .map(|triangle| {
                [
                    triangle[0] as usize,
                    triangle[1] as usize,
                    triangle[2] as usize,
                ]
            })
            .collect();

        let mesh = threemf::TriangleMesh {
            vertices,
            triangles,
        };

        threemf::write(path, &mesh)?;

        return Ok(());
    }

    let watcher = model.load_and_watch(parameters)?;

    let event_loop = EventLoop::new();
    let window = Window::new(&event_loop);

    let mut previous_time = Instant::now();

    let mut input_handler = input::Handler::new(previous_time);
    let mut renderer = block_on(Renderer::new(&window))?;

    let mut draw_config = DrawConfig::default();

    let mut shape = None;
    let mut camera = None;

    event_loop.run(move |event, _, control_flow| {
        trace!("Handling event: {:?}", event);

        let mut actions = input::Actions::new();

        let now = Instant::now();

        if let Some(new_shape) = watcher.receive() {
            let new_shape = shape_processor.process(&new_shape);
            new_shape.update_geometry(&mut renderer);

            if camera.is_none() {
                camera = Some(Camera::new(&new_shape.aabb));
            }

            shape = Some(new_shape);
        }

        //

        if let Event::WindowEvent {
            event: window_event,
            ..
        } = &event
        {
            //
            // Note: In theory we could/should check if `egui` wants "exclusive" use
            //       of this event here.
            //
            //       But with the current integration with Fornjot we're kinda blurring
            //       the lines between "app" and "platform", so for the moment we pass
            //       every event to both `egui` & Fornjot.
            //
            //       The primary visible impact of this currently is that if you drag
            //       a title bar that overlaps the model then both the model & window
            //       get moved.
            //
            //       We could also consider:
            //
            //        * Restricting `egui`'s view of the screen to e.g. 25% of actual window width.
            //
            //        * Use of <https://docs.rs/egui/0.17.0/egui/struct.Context.html#method.is_pointer_over_area>.
            //
            //        * Use of <https://docs.rs/egui/0.17.0/egui/struct.Context.html#method.wants_pointer_input>.
            //
            //        * Use of <https://docs.rs/egui/0.17.0/egui/struct.Context.html#method.is_using_pointer>.
            //
            //        * Use of <https://docs.rs/egui/0.17.0/egui/struct.Context.html#method.wants_keyboard_input>.
            //
            // TODO: Revisit this.
            //
            // TODO: Encapsulate the egui state/context access better.
            //
            renderer
                .egui_state
                .on_event(&renderer.egui_context, &window_event);
        }

        //

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }
            Event::WindowEvent {
                event: WindowEvent::Resized(size),
                ..
            } => {
                renderer.handle_resize(size);
            }
            Event::WindowEvent {
                event: WindowEvent::KeyboardInput { input, .. },
                ..
            } => {
                input_handler.handle_keyboard_input(input, &mut actions);
            }
            Event::WindowEvent {
                event: WindowEvent::CursorMoved { position, .. },
                ..
            } => {
                if let Some(camera) = &mut camera {
                    input_handler
                        .handle_cursor_moved(position, camera, &window);
                }
            }
            Event::WindowEvent {
                event: WindowEvent::MouseInput { state, button, .. },
                ..
            } => {
                if let (Some(shape), Some(camera)) = (&shape, &camera) {
                    let focus_point = camera.focus_point(
                        &window,
                        input_handler.cursor(),
                        &shape.triangles,
                    );

                    input_handler.handle_mouse_input(
                        button,
                        state,
                        focus_point,
                    );
                }
            }
            Event::WindowEvent {
                event: WindowEvent::MouseWheel { delta, .. },
                ..
            } => {
                input_handler.handle_mouse_wheel(delta, now);
            }
            Event::MainEventsCleared => {
                let delta_t = now.duration_since(previous_time);
                previous_time = now;

                if let (Some(shape), Some(camera)) = (&shape, &mut camera) {
                    input_handler.update(
                        delta_t.as_secs_f64(),
                        now,
                        camera,
                        &window,
                        &shape.triangles,
                    );
                }

                window.inner().request_redraw();
            }
            Event::RedrawRequested(_) => {
                if let (Some(shape), Some(camera)) = (&shape, &mut camera) {
                    camera.update_planes(&shape.aabb);

                    //
                    // It seems like this should be able to be done without passing the
                    // window directly--especially given how the value is used
                    // in `take_egui_input`.
                    //
                    // TODO: Revisit this.
                    //
                    match renderer.draw(
                        camera,
                        &mut draw_config,
                        &window.inner(),
                    ) {
                        Ok(()) => {}
                        Err(err) => {
                            panic!("Draw error: {}", err);
                        }
                    }
                }
            }
            _ => {}
        }

        if actions.exit {
            *control_flow = ControlFlow::Exit;
        }
        if actions.toggle_model {
            draw_config.draw_model = !draw_config.draw_model;
        }
        if actions.toggle_mesh {
            draw_config.draw_mesh = !draw_config.draw_mesh;
        }
        if actions.toggle_debug {
            draw_config.draw_debug = !draw_config.draw_debug;
        }
    });
}

struct ShapeProcessor {
    tolerance: Option<Scalar>,
}

impl ShapeProcessor {
    fn new(tolerance: Option<f64>) -> anyhow::Result<Self> {
        if let Some(tolerance) = tolerance {
            if tolerance <= 0. {
                anyhow::bail!(
                    "Invalid user defined model deviation tolerance: {}.\n\
                    Tolerance must be larger than zero",
                    tolerance
                );
            }
        }

        let tolerance = tolerance.map(Scalar::from_f64);

        Ok(Self { tolerance })
    }

    fn process(&self, shape: &fj::Shape) -> ProcessedShape {
        let aabb = shape.bounding_volume();

        let tolerance = match self.tolerance {
            None => {
                // Compute a reasonable default for the tolerance value. To do
                // this, we just look at the smallest non-zero extent of the
                // bounding box and divide that by some value.
                let mut min_extent = Scalar::MAX;
                for extent in aabb.size().components {
                    if extent > Scalar::ZERO && extent < min_extent {
                        min_extent = extent;
                    }
                }

                // `tolerance` must not be zero, or we'll run into trouble.
                let tolerance = min_extent / Scalar::from_f64(1000.);
                assert!(tolerance > Scalar::ZERO);

                tolerance
            }
            Some(user_defined_tolerance) => user_defined_tolerance,
        };

        let mut debug_info = DebugInfo::new();
        let mut triangles = Vec::new();
        triangulate(
            shape.to_shape(tolerance, &mut debug_info),
            tolerance,
            &mut triangles,
            &mut debug_info,
        );

        ProcessedShape {
            aabb,
            triangles,
            debug_info,
        }
    }
}

struct ProcessedShape {
    aabb: Aabb<3>,
    triangles: Vec<Triangle<3>>,
    debug_info: DebugInfo,
}

impl ProcessedShape {
    fn update_geometry(&self, renderer: &mut Renderer) {
        renderer.update_geometry(
            (&self.triangles).into(),
            (&self.debug_info).into(),
            self.aabb,
        );
    }
}
