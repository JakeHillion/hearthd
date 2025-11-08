use super::ast::*;

use chumsky::prelude::*;

pub fn parser<'src>() -> impl Parser<'src, &'src str, AST<'src>> {
    let inputs = parse_inputs().parse();

    just('{').map(|_c| AST::Automation {inputs: Inputs{}, conditions: Conditions{}, body: Body{}, _ph: core::marker::PhantomData})
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parse_minimal_automation() {
        let input = r#"
            { ... }
            ||:
            {}
        "#;
        let expected = AST::Automation {inputs: Inputs{}, conditions: Conditions{}, body: Body{}, _ph: core::marker::PhantomData};

        let res = parser().parse(input).unwrap();

        assert_eq!(res, expected);
    }
}
