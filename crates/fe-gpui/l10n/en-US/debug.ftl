# The debugger's marks over the line numbers (docs/design/design-system.md,
# Debugger): each row's tooltip and accessible name, `$line` its line number
# from 1, and the breakpoint menu's name.

gpui-debug-add-breakpoint = Add breakpoint, line { $line }
gpui-debug-breakpoint = Breakpoint, line { $line }
## `$message` is the debug adapter's reason, in its own words.
gpui-debug-breakpoint-unbound = Breakpoint, line { $line }, not bound: { $message }
gpui-debug-breakpoint-unbound-no-message = Breakpoint, line { $line }, not bound
gpui-debug-breakpoint-disabled = Disabled breakpoint, line { $line }
gpui-debug-stopped-here = Stopped here, line { $line }
gpui-debug-caller-frame = Caller frame, line { $line }
gpui-debug-breakpoint-menu = Breakpoint
