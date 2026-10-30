# Hook dispatch

How callbacks are registered on a named hook and later run. Implemented by `crates/cms-hooks`
(`contracts/entries/hook-dispatch.toml`). Provenance: `wp-includes/plugin.php` and
`wp-includes/class-wp-hook.php` in the pinned reference (inventory names only).

## Registration

`add` stores a live callable on a hook name, a signed integer priority (default **10**), and how
many arguments the callback opted to receive (default **1**). The name is an exact byte string;
`the_content` and `The_Content` are different hooks. A null priority in the PHP binding becomes **0**.

Lower priorities run first. Callbacks that share a priority run in the order they were first stored
at that priority. Storing the same callable again at the same priority replaces the entry in place
(accepted-argument count is updated; position is kept). Storing it at another priority is a second
entry.

`remove` needs the same callable and the same priority; it reports whether that entry existed.
`remove_all` clears one priority or every priority on the hook.

A string callable matches another string with the same bytes. An object callable matches by object
identity. Other values can be stored; they do not match a later `remove`.

## Apply and action

`apply` increments that hook's filter count even when nothing is registered, then runs callbacks.
The first argument is the value being filtered; further arguments are extras. Before each filter
callback the current value is written back as argument 0. A callback that accepted 0 arguments is
invoked with none; one that accepted fewer than were passed receives a prefix; one that accepted
as many or more receives every argument. The callback's return is the next value.

`action` increments that hook's action count and runs the same walk, but does not write returns
back onto argument 0.

If the hook named `all` has callbacks, they run first and receive the target hook's name as
argument 0, then the same arguments the target will see. Their returns do not replace the filtered
value.

While a hook is running, `current` is that hook's name (the target name, not `all`). Nested apply
or action pushes another name. `doing(name)` is true when that name is on the stack.

A pass snapshots the priority list at start and snapshots each priority's callback list when that
priority begins. A callback added at the same priority during the pass does not run in it. A
callback removed from a later priority before that priority starts does not run. Adding or
removing a *new* priority while a nested pass is live is not specified in this slice (the
reference reseats the running priority cursor).

## Counts and presence

`filter_count` / `action_count` are how many times that hook was applied or acted, including
empty walks. `has` is true when any callback is stored. `has_at` is true when that callable is
stored at that priority.
