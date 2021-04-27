use std::convert::Infallible;

use nalgebra::Point3;

use crate::geometry::{
    conversions::ToPolygon,
    operations::linear_extrude::LinearExtrude,
    shapes::Polygon,
    triangulation::brute_force::{self, triangulate, InternalError},
    Mesh,
};

pub trait ToMesh {
    type Error;

    fn to_mesh(self, tolerance: f32) -> Result<Mesh, Self::Error>;
}

impl ToMesh for Mesh {
    type Error = Infallible;

    fn to_mesh(self, _tolerance: f32) -> Result<Mesh, Self::Error> {
        Ok(self)
    }
}

impl<T> ToMesh for T
where
    T: ToPolygon,
{
    type Error = brute_force::InternalError;

    fn to_mesh(self, tolerance: f32) -> Result<Mesh, Self::Error> {
        let polygon = self.to_polygon(tolerance);
        polygon_to_mesh(polygon, 0.0)
    }
}

impl<Sketch> ToMesh for LinearExtrude<Sketch>
where
    Sketch: ToPolygon,
{
    type Error = InternalError;

    fn to_mesh(self, tolerance: f32) -> Result<Mesh, Self::Error> {
        let sketch = self.sketch.to_polygon(tolerance);

        let mut lower = polygon_to_mesh(sketch.clone(), 0.0)?;
        let upper = polygon_to_mesh(sketch.clone(), self.height)?;

        // TASK: Invert triangles of `lower` so they point down, which is the
        //       outside direction.

        // Merge meshes.
        for [a, b, c] in upper.triangles() {
            lower.triangle(a.position, b.position, c.position);
        }

        // TASK: Go through polygon vertices, connect them with their
        //       counterparts to form triangles.

        Ok(lower)
    }
}

fn polygon_to_mesh(
    polygon: Polygon,
    z: f32,
) -> Result<Mesh, brute_force::InternalError> {
    let mut mesh = Mesh::new();
    let triangles = triangulate(polygon)?;

    for triangle in triangles {
        let a_x: f32 = triangle.a.x.into();
        let a_y: f32 = triangle.a.y.into();
        let b_x: f32 = triangle.b.x.into();
        let b_y: f32 = triangle.b.y.into();
        let c_x: f32 = triangle.c.x.into();
        let c_y: f32 = triangle.c.y.into();

        mesh.triangle(
            Point3::new(a_x, a_y, z),
            Point3::new(b_x, b_y, z),
            Point3::new(c_x, c_y, z),
        );
    }

    Ok(mesh)
}
