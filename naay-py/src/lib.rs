use std::collections::BTreeMap;

use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList, PyModule, PyString};

use naay_core::{dump_naay, parse_naay, YamlNode, YamlValue};

fn yaml_to_py(py: Python<'_>, v: &YamlValue) -> PyResult<Py<PyAny>> {
    match v {
        YamlValue::Str(s) => Ok(PyString::new(py, s).unbind().into()),
        YamlValue::Seq(seq) => {
            let list = PyList::empty(py);
            for item in seq {
                list.append(yaml_to_py(py, &item.value)?)?;
            }
            Ok(list.unbind().into())
        }
        YamlValue::Map(map) => {
            let dict = PyDict::new(py);
            for (k, v2) in map {
                dict.set_item(k, yaml_to_py(py, &v2.value)?)?;
            }
            Ok(dict.unbind().into())
        }
    }
}

fn py_to_yaml(value: &Bound<'_, PyAny>) -> PyResult<YamlValue> {
    if let Ok(s) = value.cast::<PyString>() {
        Ok(YamlValue::Str(s.to_str()?.to_owned()))
    } else if let Ok(seq) = value.cast::<PyList>() {
        let mut out = Vec::new();
        for item in seq.iter() {
            out.push(YamlNode::new(py_to_yaml(&item)?));
        }
        Ok(YamlValue::Seq(out))
    } else if let Ok(dict) = value.cast::<PyDict>() {
        let mut map = BTreeMap::new();
        for (k, v2) in dict.iter() {
            let key = k.cast::<PyString>()?.to_str()?.to_owned();
            map.insert(key, YamlNode::new(py_to_yaml(&v2)?));
        }
        Ok(YamlValue::Map(map))
    } else {
        Err(pyo3::exceptions::PyTypeError::new_err(
            "Unsupported Python type for naay (expected str, list, or dict)",
        ))
    }
}

#[pyfunction]
fn loads(py: Python<'_>, s: &str) -> PyResult<Py<PyAny>> {
    let value = parse_naay(s)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("parse error: {e}")))?;
    yaml_to_py(py, &value)
}

#[pyfunction]
fn dumps(obj: Bound<'_, PyAny>) -> PyResult<String> {
    let value = py_to_yaml(&obj)?;
    dump_naay(&value)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("dump error: {e}")))
}

#[pymodule]
fn _naay_native(_py: Python<'_>, m: Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(loads, &m)?)?;
    m.add_function(wrap_pyfunction!(dumps, &m)?)?;
    Ok(())
}
