# Builds the tool and the recompiled game on Windows, from a clone of this
# repository on a Windows drive.
#
#   powershell -ExecutionPolicy Bypass -File tools\windows\build.ps1 -GameDir C:\path\to\game
#
# -GameDir      the reference copy of the game laid out as an installation
#               (the directory that holds toy2.exe and data\): your own copy.
# -LanguageDir  optionally, the game in another language (Italian, French or
#               Spanish), laid out the same way: the game is then built in it.
# -OtherDiscs   optionally, further game directories, separated by ';', whose
#               typefaces' signs go into the one atlas.
#
# Nothing of the game is copied or changed; what is written from it (the
# game's code as Rust, the program built from it) stays under out\, which
# git ignores and which must never be published.
#
# Needs Rust (https://rustup.rs) and a linker, which depends on the
# toolchain:
#   stable-x86_64-pc-windows-gnu      brings its own, but uses an
#                                     x86_64-w64-mingw32-gcc found in PATH
#                                     instead, and fails if that one is
#                                     llvm-mingw (no libgcc);
#   stable-x86_64-pc-windows-gnullvm  for llvm-mingw in PATH (the one this
#                                     script has been run with);
#   stable-x86_64-pc-windows-msvc     needs the Visual Studio build tools
#                                     (never tried here).
# To choose one for this directory only:  rustup override set <toolchain>

param(
    [Parameter(Mandatory = $true)][string]$GameDir,
    [string]$LanguageDir = "",
    [string]$OtherDiscs = ""
)

$ErrorActionPreference = "Stop"
$repository = Resolve-Path (Join-Path $PSScriptRoot "..\..")
Set-Location $repository

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Error "cargo was not found. Install Rust from https://rustup.rs and open a new terminal."
}
if (-not (Test-Path (Join-Path $GameDir "toy2.exe"))) {
    Write-Error "$GameDir does not hold toy2.exe."
}

$env:TS2_GAME_DIR = (Resolve-Path $GameDir).Path
$language = @()
if ($LanguageDir -ne "") {
    $env:TS2_LANGUAGE_DIR = (Resolve-Path $LanguageDir).Path
    $language = @("--with", "language")
}
if ($OtherDiscs -ne "") { $env:TS2_OTHER_DISCS = $OtherDiscs }

function Step([string]$what, [scriptblock]$command) {
    Write-Host "`n== $what" -ForegroundColor Cyan
    & $command
    if ($LASTEXITCODE -ne 0) { Write-Error "$what failed (exit code $LASTEXITCODE)." }
}

Step "Building the tool" { cargo build --release --bin andy }
Step "Writing the game's code as Rust" { & target\release\andy.exe recompile --out out\recomp @language }
Step "Compiling the game" { cargo build --release --manifest-path out\recomp\Cargo.toml }

Write-Host "`nDone. To play (F9 shows the gauge):" -ForegroundColor Green
Write-Host "  `$env:TS2_GAME_DIR = '$($env:TS2_GAME_DIR)'"
if ($LanguageDir -ne "") { Write-Host "  `$env:TS2_LANGUAGE_DIR = '$($env:TS2_LANGUAGE_DIR)'" }
Write-Host "  out\recomp\target\release\andy-game.exe play $($language -join ' ')"
