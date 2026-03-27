use anyhow::Result;
use anyhow::{Context, anyhow};
use std::fmt;
use std::str::FromStr;
use strum::{Display, EnumString};

/// All supported ebuild functions that can be called from the ebuild process.
#[derive(EnumString, Display, PartialEq)]
#[cfg_attr(test, derive(Debug))]
pub enum FuncType {
    // Internal ebuild functions
    #[strum(serialize = "__resolve_eclass")]
    ResolveEclass,

    // Misc Commands
    #[strum(serialize = "contains_word")]
    ContainsWord,
    #[strum(serialize = "debug-print")]
    DebugPrint,
    #[strum(serialize = "die")]
    Die,

    // PMS 12.3.13 text list functions
    #[strum(serialize = "has")]
    Has,
    #[strum(serialize = "hasv")]
    HasV,
    #[strum(serialize = "hasq")]
    HasQ,

    // PMS 12.3.14 version functions
    #[strum(serialize = "ver_cut")]
    VerCut,
    #[strum(serialize = "ver_rs")]
    VerRs,
    #[strum(serialize = "ver_test")]
    VerTest,
}

/// Holds a function call from the ebuild process,
/// consisting of a `FuncType` and its arguments as `Vec<String>`.
#[cfg_attr(test, derive(Debug))]
pub struct FuncCall {
    pub func: FuncType,
    pub args: Vec<String>,
}

impl FuncCall {
    /// Creates a new [`FuncCall`] from raw string data.
    ///
    /// Returns an `Err` if the function name cannot be resolved to a [`FuncType`].
    pub fn from_raw(func: &str, args: &[&str]) -> Result<Self> {
        let func = FuncType::from_str(func)
            .with_context(|| anyhow!("unable to resolve function for '{func}'"))?;
        let args = args.iter().map(ToString::to_string).collect();
        Ok(Self { func, args })
    }
}

impl fmt::Display for FuncCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.func, self.args.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_func_call_from_raw() {
        // (func, args), (expected func, expected args)
        let test_data = [
            (
                ("__resolve_eclass", vec!["toolchain-funcs"]),
                (FuncType::ResolveEclass, vec!["toolchain-funcs"]),
            ),
            (
                ("debug-print", vec!["*** Multiple Inheritance (Level: 2)"]),
                (
                    FuncType::DebugPrint,
                    vec!["*** Multiple Inheritance (Level: 2)"],
                ),
            ),
            (
                ("contains_word", vec!["foo", "foobar"]),
                (FuncType::ContainsWord, vec!["foo", "foobar"]),
            ),
        ];

        for ((func, args), (expected_func, expected_args)) in test_data {
            let call = FuncCall::from_raw(func, &args).unwrap();
            assert_eq!(call.func, expected_func);
            assert_eq!(call.args, expected_args);
        }
    }

    #[test]
    fn test_func_call_display() {
        let call = FuncCall {
            func: FuncType::Has,
            args: vec!["foo".to_owned(), "bar".to_owned()],
        };
        assert_eq!(call.to_string(), "has foo bar");
    }
}
