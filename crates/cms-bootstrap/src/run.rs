//! Load planned plugins and fire the matching actions.

use cms_request::Request;
use cms_types::HookName;
use php_value::{Invoke, PhpString, Slot, Value};

use crate::error::BootError;
use crate::plan::{LoadKind, LoadStep};

/// How the orchestrator includes one plugin file.
pub trait LoadPlugin {
    /// Why the file could not be included.
    type Error;

    /// Includes `path` as the engine includes a plugin file.
    ///
    /// # Errors
    ///
    /// [`Self::Error`] when the file cannot be read or run.
    fn load(&mut self, path: &[u8]) -> Result<(), Self::Error>;
}

/// Loads each step, then fires the matching action on `request`.
///
/// After must-use and network files, `muplugins_loaded` fires. After site and hacks files,
/// `plugins_loaded` fires. Both fire even when their group was empty.
///
/// # Errors
///
/// [`BootError::Load`] when a file cannot be included; [`BootError::Invoke`] when an action
/// cannot run.
pub fn run<L, I>(
    request: &Request,
    steps: &[LoadStep],
    loader: &mut L,
    invoke: &mut I,
) -> Result<(), BootError<L::Error, I::Error>>
where
    L: LoadPlugin,
    I: Invoke + ?Sized,
{
    let (early, late): (Vec<&LoadStep>, Vec<&LoadStep>) = steps
        .iter()
        .partition(|step| matches!(step.kind, LoadKind::MustUse | LoadKind::Network));
    load_group(request, &early, loader, invoke)?;
    bare_action(request, &HookName::from("muplugins_loaded"), invoke).map_err(BootError::Invoke)?;
    load_group(request, &late, loader, invoke)?;
    bare_action(request, &HookName::from("plugins_loaded"), invoke).map_err(BootError::Invoke)?;
    Ok(())
}

fn load_group<L, I>(
    request: &Request,
    steps: &[&LoadStep],
    loader: &mut L,
    invoke: &mut I,
) -> Result<(), BootError<L::Error, I::Error>>
where
    L: LoadPlugin,
    I: Invoke + ?Sized,
{
    for step in steps {
        loader.load(&step.hook_path).map_err(BootError::Load)?;
        path_action(request, &loaded_hook(step.kind), &step.hook_path, invoke)
            .map_err(BootError::Invoke)?;
    }
    Ok(())
}

fn loaded_hook(kind: LoadKind) -> HookName {
    match kind {
        LoadKind::MustUse => HookName::from("mu_plugin_loaded"),
        LoadKind::Network => HookName::from("network_plugin_loaded"),
        LoadKind::Site | LoadKind::Hacks => HookName::from("plugin_loaded"),
    }
}

fn path_action<I: Invoke + ?Sized>(
    request: &Request,
    name: &HookName,
    path: &[u8],
    invoke: &mut I,
) -> Result<(), I::Error> {
    request.hooks().action(
        name,
        &mut [Slot::from(Value::String(PhpString::new(path)))],
        invoke,
    )
}

fn bare_action<I: Invoke + ?Sized>(
    request: &Request,
    name: &HookName,
    invoke: &mut I,
) -> Result<(), I::Error> {
    request.hooks().action(name, &mut [], invoke)
}
