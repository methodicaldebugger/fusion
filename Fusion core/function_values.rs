//! Function names used as values.
//!
//! A bare identifier that is not a variable but names a function (`array_map(xs, double)`) is a
//! function value, as is an anonymous function expression (`fn(x: num) -> num => x * 2`, which the
//! parser hoists into an ordinary private function named `__lambda_N` of the enclosing module).
//!
//! In a multi-module project every function is renamed to an internal symbol that encodes its
//! module (see `module_system::mangle_function`). A function value is resolved relative to the
//! function that is currently running (or being checked): first the name as written, then the same
//! name in that function's module, then in the entry module (`crate`), which is where `main` runs.

use crate::module_system::mangle_function;

/// Split an internal function symbol into `(module, function)`.
pub fn split_symbol(symbol: &str) -> Option<(String, String)> {
    fn decode(hex: &str) -> Option<String> {
        if hex.len() % 2 != 0 || !hex.is_ascii() {
            return None;
        }
        let bytes: Option<Vec<u8>> = (0..hex.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).ok())
            .collect();
        String::from_utf8(bytes?).ok()
    }

    let rest = symbol.strip_prefix("__fusion_m")?;
    let (module_length, rest) = rest.split_once('_')?;
    let module_hex_length = module_length.parse::<usize>().ok()?.checked_mul(2)?;
    if !rest.is_ascii() || rest.len() <= module_hex_length {
        return None;
    }
    let (module_hex, rest) = rest.split_at(module_hex_length);
    let rest = rest.strip_prefix("_f")?;
    let (function_length, function_hex) = rest.split_once('_')?;
    function_length.parse::<usize>().ok()?;
    Some((decode(module_hex)?, decode(function_hex)?))
}

/// The module encoded in an internal function symbol, if `symbol` is one.
pub fn module_of_symbol(symbol: &str) -> Option<String> {
    split_symbol(symbol).map(|(module, _)| module)
}

/// A readable name for a function symbol (the declared name for internal symbols).
pub fn display_name(symbol: &str) -> String {
    match split_symbol(symbol) {
        Some((_, function)) => function,
        None => symbol.to_string(),
    }
}

/// Symbols to try, in order, for `name` used as a function value inside `current_symbol`.
pub fn function_value_candidates(current_symbol: Option<&str>, name: &str) -> Vec<String> {
    let mut candidates = vec![name.to_string()];
    if let Some(module) = current_symbol.and_then(module_of_symbol) {
        candidates.push(mangle_function(&module, name));
    }
    candidates.push(mangle_function("crate", name));
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_round_trip_module_and_function() {
        let symbol = mangle_function("shapes.geometry", "area");
        assert_eq!(
            split_symbol(&symbol),
            Some(("shapes.geometry".to_string(), "area".to_string()))
        );
        assert_eq!(module_of_symbol(&symbol), Some("shapes.geometry".to_string()));
        assert_eq!(display_name(&symbol), "area");
    }

    #[test]
    fn non_symbols_are_left_alone() {
        assert_eq!(split_symbol("area"), None);
        assert_eq!(split_symbol("__fusion_m3_616263"), None);
        assert_eq!(split_symbol("__fusion_mx_616263_f1_61"), None);
        assert_eq!(display_name("area"), "area");
    }

    #[test]
    fn candidates_prefer_the_current_module_over_the_entry_module() {
        let current = mangle_function("lib", "run");
        let candidates = function_value_candidates(Some(&current), "helper");
        assert_eq!(
            candidates,
            vec![
                "helper".to_string(),
                mangle_function("lib", "helper"),
                mangle_function("crate", "helper"),
            ]
        );
        let entry_only = function_value_candidates(None, "helper");
        assert_eq!(entry_only.len(), 2);
    }
}
