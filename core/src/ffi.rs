// #[cfg(feature = "extionsion-module")]
// use pyo3::prelude::*;
// use crate::geodesy::calibrator::calibrate_grid;
// use crate::models::Point2D;

// #[cfg(feature = "extension-module")]
// #[pyfunction]
// fn py_calibrate_stub() -> PyResult<String> {
//     let p1 = Point2D = { x: 0.0, y: 0.0 };
//     let p2 = Point2D = { x: 1.0, y: 1.0 };
//     let res = calibrate_grid_stub(p1, p2);

//     Ok(format!(:"False Easting: {}, False Northing: {}", res.false_easting_x0, res.false_northing_y0))
// }

// #[cfg(feature = "extension-module")]
// #[pymodule]
// fn geo_core(_py: Python, m: &PyModule) -> PyResult<()> {
//     m.add_function(wrap_pyfunction!(py_calibrate_stub)?)?;
//     Ok(())
// }
