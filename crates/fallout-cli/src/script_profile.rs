//! Build decoder inputs from verified executable metadata. The data crate has
//! no dependency on an executable path, fixed address or inspection frontend.
use super::{Result, command_catalogue::Catalogue};
use fallout_data::obscript::{
    argument_census::{CommandSignature, Signatures},
    arguments::{Convention, Parameter},
    expression::{Operator, Operators},
};

pub(super) fn operators(catalogue: &Catalogue) -> Result<Operators> {
    Ok(Operators::new(
        catalogue
            .operators
            .iter()
            .map(|row| Operator {
                code: row.code,
                precedence: row.precedence,
                spelling: row.spelling.as_bytes().to_vec(),
            })
            .collect(),
    )?)
}

pub(super) fn signatures(catalogue: &Catalogue) -> Signatures {
    catalogue
        .script_commands
        .iter()
        .map(|row| {
            let convention = match row.parse_convention {
                "vanilla-default" => Convention::Default,
                "vanilla-message" => Convention::Message,
                _ => Convention::Unknown,
            };
            (
                row.id as u16,
                CommandSignature {
                    convention,
                    parameters: row
                        .parameters
                        .iter()
                        .map(|param| Parameter {
                            type_id: param.type_id,
                            optional_word: param.optional_word,
                        })
                        .collect(),
                },
            )
        })
        .collect()
}
