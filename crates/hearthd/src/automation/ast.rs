use core::marker::PhantomData;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Automation<'src> {
    inputs: Inputs<'src>,
    conditions: Conditions,
    body: Body,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Inputs<'src> {
        _ph: PhantomData<&'src bool>
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Conditions {}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Body {}
