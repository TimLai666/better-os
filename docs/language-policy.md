# Language Policy

- Prefer Rust for GUIs, system services, low-level collectors, file
  operations, performance-sensitive code, and long-running daemons.
- Use Go only when networking, remote management, agents, or standalone service
  tooling has a concrete advantage. Record that reason in an ADR.
- Do not add new first-party C or C++ code.
- Do not use Python, JavaScript, Ruby, Electron, Tauri, or a browser frontend
  for production components.
- Shell is limited to packaging and bootstrap glue. Core behavior belongs in
  Rust or an explicitly justified Go component.
- Existing Linux libraries implemented in C are acceptable dependencies when
  required by the platform.
