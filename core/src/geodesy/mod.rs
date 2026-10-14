pub mod calibrator;

use crate::models::{CalibrationResult, UniversalCalibrationRequest};

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum CalibrationError {
    #[error("Not enough control points. Provided: {provided}, required: {required}")]
    InsufficientPoints { provided: usize, required: usize },

    #[error("Invalid geometry: points overlap or configuration is linear")]
    InvalidGeometry,

    #[error("Projection engine error: {0}")]
    ProjectionError(String),

    #[error("Failed to calibrate system within specified tolerance")]
    ToleranceNotMet,

    #[error("Unsupported transformation combination")]
    UnsupportedTransformation,
}

pub trait UniversalCalibrator {
    fn calibrate(
        &self,
        request: UniversalCalibrationRequest,
    ) -> Result<CalibrationResult, CalibrationError>;
}
