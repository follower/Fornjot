//! API for processing shapes

use fj_interop::{debug::DebugInfo, mesh::Mesh};
use fj_kernel::{
    algorithms::{triangulate, InvalidTolerance, Tolerance},
    shape::ValidationError,
};
use fj_math::{Aabb, Point, Scalar};

use crate::ToShape as _;

/// Processes an [`fj::Shape`] into a [`ProcessedShape`]
pub struct ShapeProcessor {
    /// The tolerance value used for creating the triangle mesh
    pub tolerance: Option<Tolerance>,
}

impl ShapeProcessor {
    /// Process an [`fj::Shape`] into [`ProcessedShape`]
    pub fn process(&self, shape: &fj::Shape) -> Result<ProcessedShape, Error> {
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

                let tolerance = min_extent / Scalar::from_f64(1000.);
                Tolerance::from_scalar(tolerance)?
            }
            Some(user_defined_tolerance) => user_defined_tolerance,
        };

        let mut debug_info = DebugInfo::new();
        let mesh = triangulate(
            shape.to_shape(tolerance, &mut debug_info)?,
            tolerance,
            &mut debug_info,
        );

        Ok(ProcessedShape {
            aabb,
            mesh,
            debug_info,
        })
    }
}

/// A processed shape
///
/// Created by [`ShapeProcessor::process`].
pub struct ProcessedShape {
    /// The axis-aligned bounding box of the shape
    pub aabb: Aabb<3>,

    /// The triangle mesh that approximates the original shape
    pub mesh: Mesh<Point<3>>,

    /// The debug info generated while processing the shape
    pub debug_info: DebugInfo,
}

/// A shape processing error
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error converting to shape
    #[error("Error converting to shape")]
    ToShape(#[from] ValidationError),

    /// Model has zero size
    #[error("Model has an zero size")]
    Extent(#[from] InvalidTolerance),
}
