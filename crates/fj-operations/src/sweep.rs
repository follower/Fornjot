use fj_interop::debug::DebugInfo;
use fj_kernel::{
    algorithms::{sweep, Tolerance},
    objects::Face,
    validation::{validate, Validated, ValidationConfig, ValidationError},
};
use fj_math::{Aabb, Vector};

use super::ToShape;

impl ToShape for fj::Sweep {
    fn to_shape(
        &self,
        config: &ValidationConfig,
        tolerance: Tolerance,
        debug_info: &mut DebugInfo,
    ) -> Result<Validated<Vec<Face>>, ValidationError> {
        let sketch = self.shape().to_shape(config, tolerance, debug_info)?;
        let path = Vector::from(self.path());
        let color = self.shape().color();

        let solid = sweep(sketch.into_inner(), path, tolerance, color);

        validate(solid, config)
    }

    fn bounding_volume(&self) -> Aabb<3> {
        self.shape()
            .bounding_volume()
            .merged(&Aabb::<3>::from_points(
                self.shape()
                    .bounding_volume()
                    .vertices()
                    .map(|v| v + self.path()),
            ))
    }
}
