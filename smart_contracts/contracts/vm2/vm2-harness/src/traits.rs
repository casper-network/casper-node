use casper_macros::casper;

#[casper(trait_definition)]
pub trait Fallback {
    #[casper(selector(fallback))]
    fn fallback(&mut self);
}
