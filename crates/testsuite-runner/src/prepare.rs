//! `prepare-fdr`: makes RoboTool's generated CSP checkable by FDR. The
//! preparation (finite bounds, function bodies, value and binder domains) is
//! built from the generator's facts about the project's `source-data/`
//! snapshot (`testsuite_generator::facts`); `robotool_harness::prepare`
//! applies it to the CSP.

use robotool_harness::{BinderDomain, DomainType, FdrPreparation, FunctionInstance, ValueDomain};
use std::path::Path;
use testsuite_generator::Result;
use testsuite_generator::facts::{self, Snapshot};

/// Prepare the compiled CSP of `project` for FDR. `refines` is FDR's `refines`
/// executable, which evaluates the model's bound expressions.
pub fn prepare_fdr(project: &Path, refines: &Path) -> Result<()> {
    let snapshot = Snapshot::load(&project.join("source-data"))?;
    robotool_harness::prepare(project, &fdr_preparation(&snapshot)?, refines)
}

/// The FDR preparation of the cases in `snapshot`. The bounds cover every row
/// of the snapshot, including rows that cannot be generated: `core_nat` also
/// covers every sequence length. The functions and domains are those of the
/// cases that can be generated.
pub(crate) fn fdr_preparation(snapshot: &Snapshot) -> Result<FdrPreparation> {
    let extents = facts::extents(snapshot)?;
    let sequence_bound = extents.sequence_length.max(2);
    let core_nat_max = extents.int_range.1.max(i64::try_from(extents.sequence_length)?).max(2);
    let mut preparation = FdrPreparation {
        core_int: extents.int_range,
        core_nat_max,
        sequence_bound,
        sizing_constants: extents.constants,
        clock_max: extents.clock_max,
        functions: Vec::new(),
        value_domains: Vec::new(),
        binder_domains: Vec::new(),
    };
    for case in facts::csp_facts(snapshot) {
        let package = case.package;
        for function in case.facts.functions {
            preparation.functions.push(FunctionInstance {
                package: package.clone(),
                name: function.name,
                parameters: function.parameters,
                body: function.body,
            });
        }
        for domain in case.facts.value_domains {
            let csp_type = domain.csp_type(&package, sequence_bound);
            let ty = if domain.is_sequence() { DomainType::Sequence(csp_type) } else { DomainType::Datatype(csp_type) };
            preparation.value_domains.push(ValueDomain { package: package.clone(), ty, values: domain.values });
        }
        for binder in case.facts.binder_domains {
            preparation.binder_domains.push(BinderDomain {
                package: package.clone(),
                binder: binder.binder,
                ty: binder.ty,
                values: binder.values,
            });
        }
    }
    Ok(preparation)
}
