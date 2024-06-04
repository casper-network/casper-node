use casper_macros::casper;

#[casper]
pub trait Fallback {
    #[casper(fallback)]
    fn fallback(&mut self);
}
