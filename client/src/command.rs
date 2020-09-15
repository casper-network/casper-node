use clap::{App, ArgMatches};

pub trait ClientCommand<'a, 'b> {
    const NAME: &'static str;
    const ABOUT: &'static str;
    /// Constructs the clap `SubCommand` and returns the clap `App`.
    fn build(display_order: usize) -> App<'a, 'b>;
    /// Parses the arg matches and runs the subcommand.
    fn run(matches: &ArgMatches<'_>);
}
