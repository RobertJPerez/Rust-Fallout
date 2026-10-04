//! Command families keep Clap's generated debug stack frames bounded.
use clap::Subcommand;

mod assets;
pub(super) use assets::{AssetsCommand, NifSkinArgs};
mod runtime;
pub(super) use runtime::RuntimeCommand;
mod actors;
pub(super) use actors::ActorsCommand;
mod scripts;
pub(super) use scripts::ScriptsCommand;
mod world;
pub(super) use world::WorldCommand;
mod physics;
pub(super) use physics::PhysicsCommand;
mod sources;
pub(super) use sources::SourcesCommand;

#[derive(Subcommand)]
pub(super) enum Command {
    #[command(flatten)]
    Assets(AssetsCommand),
    #[command(flatten)]
    Runtime(RuntimeCommand),
    #[command(flatten)]
    Actors(ActorsCommand),
    #[command(flatten)]
    Scripts(ScriptsCommand),
    #[command(flatten)]
    World(WorldCommand),
    #[command(flatten)]
    Physics(PhysicsCommand),
    #[command(flatten)]
    Sources(SourcesCommand),
}
