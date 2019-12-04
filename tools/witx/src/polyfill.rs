use crate::{Document, Id, InterfaceFunc, InterfaceFuncParam, Module, RepEquality, Representable};
use std::collections::HashMap;
use std::rc::Rc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PolyfillError {
    #[error("Module not present: {name:?}")]
    ModuleNotPresent { name: Id },
    #[error("Function not present: {name:?}")]
    FuncNotPresent { name: Id },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Polyfill {
    pub modules: Vec<ModulePolyfill>,
}

impl Polyfill {
    pub fn new(
        new: &Document,
        old: &Document,
        module_mapping: &HashMap<String, String>, // Will need a more sophisticated mapping - what about function names, argument names?
    ) -> Result<Self, PolyfillError> {
        let mut modules = Vec::new();
        for (newname, oldname) in module_mapping {
            let newname = Id::new(newname);
            let oldname = Id::new(oldname);
            let newmod = new
                .module(&newname)
                .ok_or_else(|| PolyfillError::ModuleNotPresent { name: newname })?;
            let oldmod = old
                .module(&oldname)
                .ok_or_else(|| PolyfillError::ModuleNotPresent { name: oldname })?;
            modules.push(ModulePolyfill::new(newmod, oldmod)?);
        }
        Ok(Polyfill { modules })
    }

    pub fn report(&self) -> String {
        self.modules
            .iter()
            .map(|m| m.report())
            .collect::<Vec<String>>()
            .join("\n")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModulePolyfill {
    pub new: Rc<Module>,
    pub old: Rc<Module>,
    pub funcs: Vec<FuncPolyfill>,
}

impl ModulePolyfill {
    pub fn new(new: Rc<Module>, old: Rc<Module>) -> Result<Self, PolyfillError> {
        let mut funcs = Vec::new();
        for oldfunc in old.funcs() {
            let newfunc = new
                .func(&oldfunc.name)
                .ok_or_else(|| PolyfillError::FuncNotPresent {
                    name: oldfunc.name.clone(),
                })?;
            funcs.push(FuncPolyfill::new(newfunc, oldfunc));
        }
        Ok(ModulePolyfill { new, old, funcs })
    }

    pub fn report(&self) -> String {
        format!(
            "Implement module {} in terms of {}:\n\t{}",
            self.new.name.as_str(),
            self.old.name.as_str(),
            self.funcs
                .iter()
                .map(|f| f.report())
                .collect::<Vec<String>>()
                .join("\n\t"),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuncPolyfill {
    pub new: Rc<InterfaceFunc>,
    pub old: Rc<InterfaceFunc>,
    pub mapped_params: Vec<ParamPolyfill>,
    pub unknown_params: Vec<ParamUnknown>,
    pub mapped_results: Vec<ParamPolyfill>,
    pub unknown_results: Vec<ParamUnknown>,
}

impl FuncPolyfill {
    pub fn new(new: Rc<InterfaceFunc>, old: Rc<InterfaceFunc>) -> FuncPolyfill {
        let mut mapped_params = Vec::new();
        let mut unknown_params = Vec::new();

        // Old function is called. Need to map each of its parameters to the new function:
        for old_param in old.params.iter() {
            if let Some(new_param) = new.params.iter().find(|p| p.name == old_param.name) {
                mapped_params.push(ParamPolyfill {
                    new: new_param.clone(),
                    old: old_param.clone(),
                    // Call new param type with old param:
                    repeq : old_param
                        .tref
                        .type_()
                        .representable(&new_param.tref.type_()),
                })
            } else {
                unknown_params.push(ParamUnknown::Old(old_param.clone()));
            }
        }
        // Are any new params not covered by the old params?
        // This search is O(n^2), but n ought to be small.
        for new_param in new.params.iter() {
            if mapped_params
                .iter()
                .find(|m| m.new.name == new_param.name)
                .is_none()
            {
                unknown_params.push(ParamUnknown::New(new_param.clone()));
            }
        }

        let mut mapped_results = Vec::new();
        let mut unknown_results = Vec::new();

        // New function has returned. Need to map each of its results to the old function:
        for new_result in new.results.iter() {
            if let Some(old_result) = old.results.iter().find(|p| p.name == new_result.name) {
                mapped_results.push(ParamPolyfill {
                    new: new_result.clone(),
                    old: old_result.clone(),
                    // Return new result type as old result:
                    repeq : new_result
                        .tref
                        .type_()
                        .representable(&old_result.tref.type_()),
                })
            } else {
                unknown_results.push(ParamUnknown::New(new_result.clone()));
            }
        }

        // Are any old results not covered by the new results?
        for old_result in old.results.iter() {
            if mapped_results
                .iter()
                .find(|m| m.old.name == old_result.name)
                .is_none()
            {
                unknown_results.push(ParamUnknown::Old(old_result.clone()));
            }
        }

        FuncPolyfill {
            new,
            old,
            mapped_params,
            unknown_params,
            mapped_results,
            unknown_results,
        }
    }

    pub fn report(&self) -> String {
        if self.full_compat() {
            format!("{}: full compatibility", self.new.name.as_str())
        } else {
            let name = if self.new.name != self.old.name {
                format!("{} => {}", self.old.name.as_str(), self.new.name.as_str())
            } else {
                self.new.name.as_str().to_string()
            };
            let mut contents = Vec::new();
            for p in self.mapped_params.iter() {
                contents.push(if !p.full_compat() {
                    format!("param {}", p.report())
                } else {
                    format!("param {}: compatible", p.new.name.as_str())
                })
            }
            for u in self.unknown_params.iter() {
                contents.push(format!(
                    "{} param {}: no corresponding result!",
                    u.which(),
                    u.param().name.as_str()
                ))
            }
            for r in self.mapped_results.iter() {
                contents.push(if !r.full_compat() {
                    format!("result {}", r.report())
                } else {
                    format!("result {}: compatible", r.new.name.as_str())
                })
            }
            for u in self.unknown_results.iter() {
                contents.push(format!(
                    "{} result {}: no corresponding result!",
                    u.which(),
                    u.param().name.as_str()
                ))
            }
            let contents = if contents.is_empty() {
                String::new()
            } else {
                format!(":\n\t\t{}", contents.join("\n\t\t"))
            };
            format!("{}{}", name, contents)
        }
    }
    pub fn full_compat(&self) -> bool {
        self.new.name == self.old.name
            && self.mapped_params.iter().all(|p| p.full_compat())
            && self.unknown_params.is_empty()
            && self.mapped_results.iter().all(|p| p.full_compat())
            && self.unknown_results.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamPolyfill {
    pub new: InterfaceFuncParam,
    pub old: InterfaceFuncParam,
    pub repeq: RepEquality,
}

impl ParamPolyfill {
    pub fn full_compat(&self) -> bool {
        self.new.name == self.old.name && self.repeq == RepEquality::Eq
    }
    pub fn report(&self) -> String {
        let name = if self.new.name != self.old.name {
            format!("{} => {}", self.old.name.as_str(), self.new.name.as_str())
        } else {
            self.new.name.as_str().to_string()
        };
        let repr = match self.repeq {
            RepEquality::Eq => "compatible types".to_string(),
            RepEquality::Superset => format!("{} is superset-compatible with {}", self.old.tref.type_name(), self.new.tref.type_name()),
            RepEquality::NotEq => format!("{} is incompatible with new {}", self.old.tref.type_name(), self.new.tref.type_name())
        };
        format!("{}: {}", name, repr)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamUnknown {
    Old(InterfaceFuncParam),
    New(InterfaceFuncParam),
}

impl ParamUnknown {
    pub fn which(&self) -> &'static str {
        match self {
            ParamUnknown::Old { .. } => "old",
            ParamUnknown::New { .. } => "new",
        }
    }
    pub fn param(&self) -> &InterfaceFuncParam {
        match self {
            ParamUnknown::Old(p) => &p,
            ParamUnknown::New(p) => &p,
        }
    }
}