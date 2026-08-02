use anyhow::{Context, anyhow, bail};
use std::fmt;
use std::str::FromStr;

/// All supported ebuild functions that can be called from the ebuild process.
#[derive(Debug, PartialEq)]
pub enum FuncType {
    // Internal ebuild functions
    ResolveEclass,

    // Misc Commands
    DebugPrint,
    Die,

    // PMS 12.3.13 text list functions
    Has,
    HasV,
    HasQ,

    // PMS 12.3.14 version functions
    VerCut,
    VerRs,
    VerTest,
}

impl FromStr for FuncType {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> anyhow::Result<Self> {
        match value {
            "__resolve_eclass" => Ok(Self::ResolveEclass),
            "debug-print" => Ok(Self::DebugPrint),
            "die" => Ok(Self::Die),
            "has" => Ok(Self::Has),
            "hasv" => Ok(Self::HasV),
            "hasq" => Ok(Self::HasQ),
            "ver_cut" => Ok(Self::VerCut),
            "ver_rs" => Ok(Self::VerRs),
            "ver_test" => Ok(Self::VerTest),
            _ => bail!("unknown ebuild function '{value}'"),
        }
    }
}

impl fmt::Display for FuncType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::ResolveEclass => "__resolve_eclass",
            Self::DebugPrint => "debug-print",
            Self::Die => "die",
            Self::Has => "has",
            Self::HasV => "hasv",
            Self::HasQ => "hasq",
            Self::VerCut => "ver_cut",
            Self::VerRs => "ver_rs",
            Self::VerTest => "ver_test",
        };
        f.write_str(value)
    }
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
    pub fn from_raw(func: &str, args: &[&str]) -> anyhow::Result<Self> {
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
