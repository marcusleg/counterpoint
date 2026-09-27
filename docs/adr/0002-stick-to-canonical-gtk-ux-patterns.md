# ADR-0002: Stick to canonical GTK UI/UX patterns

|        |            |
| ------ | ---------- |
| Status | accepted   |
| Date   | 2026-09-27 |

This is a policy for how Counterpoint's user interface is built. It came up when the canonical
GTK behaviour for a new menu item looked worse than a workaround could make it look.

## Decision

Counterpoint uses canonical GTK UI and UX patterns and GNOME best practice, even where the
canonical UX isn't always great, rather than workarounds or non-canonical UX that look better.

The UX will improve as GTK matures.

### Consequences

- Where GTK has no canonical way to present something as we would like, we accept the
  canonical result, rough edges included.
- A design that needs a workaround to look right is dropped in favour of what GTK
  supports.
- Before building a piece of UI, check what GTK supports natively and what GNOME
  best practice is, and say so when a requested look would need a workaround.

## Context

With **Open Recent** in the main menu and no recent files, the item opened an empty
submenu, which looked odd. The wish was to show the item greyed out instead. GTK cannot
disable an item that opens a submenu. The only way to grey it out was to swap it for a
different kind of item while the list is empty.

## Alternatives considered

### Grey out Open Recent with a workaround

While the list is empty, replace the submenu with an ordinary item tied to an action that is
never enabled, so GTK draws it greyed out. It gave the look that was wanted, but it is a
workaround rather than canonical UX.
