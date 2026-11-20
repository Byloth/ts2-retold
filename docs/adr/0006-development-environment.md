# ADR 0006 — Development environment

Status: Accepted (2026-11-20). Supersedes the environment section of the brief
where they differ.

- Development happens in WSL2. Tools are installed **per user** (rustup,
  `cargo install`, project-local directories); Docker is the fallback when
  something needs system packages. System-wide installs are avoided.
- **WSL ↔ Windows interop is disabled.** Nothing in WSL can launch a Windows
  program. Anything that must run on Windows (the original game, a native
  Windows build, instrumentation of the original) is delivered as a script or
  an exact PowerShell command for the user to run.
- mingw-w64 is installed on the Windows side, not in WSL.
- The game directory is mounted read-only in practice: tools only read it.
