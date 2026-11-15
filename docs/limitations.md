# Known limitations

- This MVP targets Windows 10/11 x64. PTY APIs are portable, but non-Windows GUI and process cleanup have no release support.
- Script launchers appear in scans as requiring an interpreter. To run one, explicitly register the approved absolute interpreter and its argument array. Launch-time verification covers the executable. Keep integrity controls for scripts, libraries and other runtime dependencies.
- The Windows process tree is put in a kill-on-close Job Object immediately after spawn. A child created before assignment can escape it. An approved process can also use external services or brokers. Treat runtime trust as granting the user’s execution permissions.
- The app supports up to 24 open terminal tabs. Terminal output is bounded by the per-tab queue and xterm scrollback. Sustained output can drop chunks, with a marker and parser reset. Closing a tab discards its terminal buffer.
- A saved session contains launch metadata. Gajae-Code resume starts a new process and opens GJC's native history picker. The selected Deck entry supplies launch context. GJC owns the history and the user chooses the native conversation. Live PTY reattachment and detached/background sessions are outside the MVP.
- Router capability toggles are operator-supplied. A successful HTTP probe is not proof of inference compatibility. GJC and Hermes provider selection is manual, with optional environment bindings to carry values documented for the installed runtime.
- Restore UI diagnostics run when the operator requests a restart. The export covers router profiles and terminal appearance. Executable trust must be established independently on every installation.

