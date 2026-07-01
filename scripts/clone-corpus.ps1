# Shallow-clone the 8 Rust/Go corpus repos for re-analysis.
$ErrorActionPreference = 'Stop'
$repos = @(
    @('ripgrep', 'https://github.com/BurntSushi/ripgrep'),
    @('tokio',   'https://github.com/tokio-rs/tokio'),
    @('tracing', 'https://github.com/tokio-rs/tracing'),
    @('serde',   'https://github.com/serde-rs/serde'),
    @('gin',     'https://github.com/gin-gonic/gin'),
    @('viper',   'https://github.com/spf13/viper'),
    @('cobra',   'https://github.com/spf13/cobra'),
    @('prometheus','https://github.com/prometheus/prometheus'),
    # G2c-5 external corpus — 3 dil çeşitliliği (JS/Python/Go).
    @('chalk',   'https://github.com/chalk/chalk'),
    @('click',   'https://github.com/pallets/click')
)
$root = 'P:\Work\repos'
New-Item -ItemType Directory -Path $root -Force | Out-Null
foreach ($r in $repos) {
    $name = $r[0]; $url = $r[1]
    $dest = Join-Path $root $name
    if (Test-Path $dest) {
        Write-Output "SKIP $name (exists)"
    } else {
        Write-Output "CLONE $name"
        git clone --depth 1 --quiet $url $dest
    }
}
Write-Output "DONE"
