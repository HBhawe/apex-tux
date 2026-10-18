#!/usr/bin/env bash
# Builds apex-tux, installs it, and (re)starts it as a systemd user service
# that is enabled to start with your graphical session.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
bin_dir="$HOME/.local/bin"
config_dir="$HOME/.config/apex-tux"
unit_dir="$HOME/.config/systemd/user"

cd "$repo"
cargo build --release

mkdir -p "$bin_dir" "$config_dir" "$unit_dir"

# Stop first so the binary isn't busy while being replaced.
systemctl --user stop apex-tux.service 2>/dev/null || true
install -m 755 target/release/apex-tux "$bin_dir/apex-tux"

# Keep an existing config; only seed it on first install.
if [ ! -f "$config_dir/settings.toml" ]; then
    cp settings.toml "$config_dir/settings.toml"
    echo "Copied default settings to $config_dir/settings.toml"
fi

cat > "$unit_dir/apex-tux.service" <<'EOF'
[Unit]
Description=apex-tux OLED display for SteelSeries Apex
After=graphical-session.target
PartOf=graphical-session.target

[Service]
Type=simple
WorkingDirectory=%h/.config/apex-tux
ExecStart=%h/.local/bin/apex-tux
Restart=on-failure
RestartSec=5

[Install]
WantedBy=graphical-session.target
EOF

systemctl --user daemon-reload
systemctl --user enable --now apex-tux.service
systemctl --user --no-pager status apex-tux.service | head -5
