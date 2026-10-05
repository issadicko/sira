use xc_script::NextRequest;

use crate::pipeline::Outcome;

/// Au-delà, un run qui saute sans cesse d'une requête à l'autre est arrêté (garde de Bruno).
pub const MAX_JUMPS: usize = 10_000;

/// La suite d'un run après une requête.
#[derive(Debug, PartialEq, Eq)]
pub struct Step {
    /// Position de la requête suivante ; `None` termine le run.
    pub next: Option<usize>,
    pub warning: Option<String>,
}

/// Où va le run après la requête `at` de `names` : à la suivante, ou là où les scripts l'ont envoyé
/// (`bru.setNextRequest(nom)` : première requête de ce nom, avant ou après ; `null` ou `stopExecution` l'arrête).
pub fn next_step(names: &[String], at: usize, outcome: &Outcome, jumps: &mut usize) -> Step {
    let done = |warning: Option<String>| Step { next: None, warning };
    if outcome.stop {
        return done(None);
    }
    match &outcome.next_request {
        NextRequest::Unset => Step { next: Some(at + 1).filter(|n| *n < names.len()), warning: None },
        NextRequest::Stop => done(None),
        NextRequest::Named(name) => {
            *jumps += 1;
            if *jumps > MAX_JUMPS {
                return done(Some("Too many jumps, possible infinite loop".into()));
            }
            match names.iter().position(|n| n == name) {
                Some(index) => Step { next: Some(index), warning: None },
                None => Step {
                    next: Some(at + 1).filter(|n| *n < names.len()),
                    warning: Some(format!("Could not find request with name '{name}'")),
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use xc_core::yaml::Map;
    use xc_core::RequestDoc;

    use super::*;

    fn names() -> Vec<String> {
        ["A", "B", "C"].map(String::from).to_vec()
    }

    fn outcome(next: NextRequest, stop: bool) -> Outcome {
        let mut out = Outcome::new(&RequestDoc::from_tree(&Map::default()));
        out.next_request = next;
        out.stop = stop;
        out
    }

    fn step(at: usize, next: NextRequest, stop: bool) -> Step {
        next_step(&names(), at, &outcome(next, stop), &mut 0)
    }

    #[test]
    fn ef_run_02_without_instruction_the_run_goes_on_then_ends() {
        assert_eq!(step(0, NextRequest::Unset, false), Step { next: Some(1), warning: None });
        assert_eq!(step(2, NextRequest::Unset, false), Step { next: None, warning: None });
    }

    #[test]
    fn ef_run_02_set_next_request_jumps_forward_or_back_to_the_first_name() {
        assert_eq!(step(0, NextRequest::Named("C".into()), false).next, Some(2));
        assert_eq!(step(2, NextRequest::Named("A".into()), false).next, Some(0));
    }

    #[test]
    fn ef_run_02_null_and_stop_execution_end_the_run() {
        assert_eq!(step(0, NextRequest::Stop, false).next, None);
        assert_eq!(step(0, NextRequest::Named("C".into()), true).next, None);
    }

    #[test]
    fn ef_run_02_an_unknown_name_warns_and_goes_on() {
        let step = step(0, NextRequest::Named("Nope".into()), false);
        assert_eq!(step.next, Some(1));
        assert_eq!(step.warning.as_deref(), Some("Could not find request with name 'Nope'"));
    }

    #[test]
    fn ef_run_02_endless_jumping_is_cut() {
        let out = outcome(NextRequest::Named("A".into()), false);
        let mut jumps = MAX_JUMPS;
        let step = next_step(&names(), 0, &out, &mut jumps);
        assert_eq!(step.next, None);
        assert!(step.warning.unwrap().contains("Too many jumps"));
    }
}
