# Engineering documents

Working documents for the people and agents developing Better OS. Documentation for the project's users is in `docs/`: the third-party license notices that ship in every package.

| Document | What it holds | Read it when |
|---|---|---|
| [architecture.md](architecture.md) | How the system is built: components, trust boundaries, data flow, where tests hook in | Before changing the structure, the tests, packaging or the privileged boundary |
| [specs/](specs/) | One file per feature or policy: what it promises, what it leaves out, how to tell it is done (Better Files policies, storage safety signals, touchpad mapping, release packaging, security and rollback, and the rest) | Before working on that feature |
| [contracts/](contracts/) | The component manifest schema that component authors write against | Before changing manifest fields or validation |
| [performance/](performance/) | Benchmark methods and recorded numbers for Better Files, Better Launcher, and Better Storage | Before making or changing a performance claim |
| [../design/](../design/) | Screen designs, each a spec with every state and its zh-TW and English copy plus HTML mockups in both appearances: the control center (`control-center-spec.md`, ticket 83), the top bar (`top-bar-spec.md`, ticket 95, with the owner's decisions of 2026-11-21 at the top), and Better Input, the input method with voice typing (`better-input-spec.md`, ticket 99, with the decisions still open for the owner at the top) | Before building or changing a screen they describe |
| [decisions/](decisions/) | One file per hard-to-reverse choice, with the options that were rejected. A proposed record waits for the owner with its decisions listed at the top, as does Better OS's desktop: GNOME, COSMIC, or a Better OS session on COSMIC's compositor (0038, ticket 143, with the desktop-icons question ticket 144 waits on and which of tickets 150 to 161 wait under each option) | Before proposing to change such a choice |
| [tickets/](tickets/) | One file per slice of work | Picking the next piece of work |
| [handoff.md](handoff.md) | Unfinished work that has no ticket. Present only while there is some | Arriving to pick up work |

## How to work with them

- **Progress is where the ticket files are.** Open tickets are in `tickets/`, tickets in progress in `tickets/doing/`, finished ones in `tickets/done/`. A ticket can start when every ticket it lists under "Blocked by" is in `done/`. The number is a name, not an order.
- **A ticket is finished by the change that delivers it.** The same change moves the ticket file to `done/`.
- **When a requirement changes, the specification changes first**, then the tickets that depend on it, then the code.
- **A problem found and not fixed now becomes a ticket.**
- **A decision record is not edited after it is accepted.** A later decision gets a new record and marks the old one as superseded. Two records share the number 0010; they are told apart by name.
