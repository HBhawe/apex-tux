//! Volume overlay: briefly shows the current volume on the OLED whenever it
//! changes, e.g. when you turn the Apex 7's volume roller.
//!
//! Volume changes are detected with `pactl subscribe`, which works with both
//! PipeWire (via pipewire-pulse) and PulseAudio. It reacts to any volume
//! change, so the tray slider and media keys show the overlay too.

use anyhow::{Context, Result};
use apex_hardware::FrameBuffer;
use embedded_graphics::{
    mono_font::{iso_8859_15, MonoTextStyle},
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Arc, Line, PrimitiveStyle, Rectangle, Triangle},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};
use log::{info, warn};
use std::process::Stdio;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::watch,
    time::{sleep, Duration},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Volume {
    pub percent: u32,
    pub muted: bool,
}

/// Starts watching the default output's volume in the background.
/// The receiver only fires on real changes, never on startup.
pub fn spawn_watcher() -> watch::Receiver<Option<Volume>> {
    let (tx, rx) = watch::channel(None);
    tokio::spawn(async move {
        loop {
            if let Err(e) = watch_pactl(&tx).await {
                warn!("Volume watcher stopped: {e:#}");
            }
            if tx.is_closed() {
                break;
            }
            // pactl exits if PipeWire restarts; try again shortly.
            sleep(Duration::from_secs(3)).await;
        }
    });
    rx
}

async fn watch_pactl(tx: &watch::Sender<Option<Volume>>) -> Result<()> {
    let mut child = Command::new("pactl")
        .arg("subscribe")
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("couldn't start `pactl subscribe` (install the libpulse package)")?;
    let stdout = child.stdout.take().context("pactl has no stdout")?;
    let mut lines = BufReader::new(stdout).lines();

    // Remember the starting volume so launching apex-tux doesn't show the overlay.
    let mut last = read_volume().await.ok();
    info!("Volume watcher started");

    while let Some(line) = lines.next_line().await? {
        // Sink events cover volume/mute; server events cover a new default output.
        if !(line.contains("on sink #") || line.contains("on server")) {
            continue;
        }
        if let Ok(volume) = read_volume().await {
            if Some(volume) != last {
                last = Some(volume);
                tx.send_replace(Some(volume));
            }
        }
    }
    Ok(())
}

async fn read_volume() -> Result<Volume> {
    // Output looks like: "Volume: front-left: 42597 /  65% / -11.23 dB, ..."
    let out = Command::new("pactl")
        .args(["get-sink-volume", "@DEFAULT_SINK@"])
        .output()
        .await?;
    let text = String::from_utf8_lossy(&out.stdout);
    let percent = text
        .split('/')
        .nth(1)
        .and_then(|s| s.trim().trim_end_matches('%').parse().ok())
        .context("couldn't parse pactl volume output")?;

    let out = Command::new("pactl")
        .args(["get-sink-mute", "@DEFAULT_SINK@"])
        .output()
        .await?;
    let muted = String::from_utf8_lossy(&out.stdout).contains("yes");

    Ok(Volume { percent, muted })
}

/// Draws a 128x40 frame: speaker icon, large percentage, and a level bar.
pub fn render(volume: Volume) -> Result<FrameBuffer> {
    let mut fb = FrameBuffer::new();
    let fill = PrimitiveStyle::with_fill(BinaryColor::On);
    let thin = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
    let thick = PrimitiveStyle::with_stroke(BinaryColor::On, 2);

    // Speaker icon
    Rectangle::new(Point::new(4, 9), Size::new(5, 8))
        .into_styled(fill)
        .draw(&mut fb)?;
    Triangle::new(Point::new(8, 13), Point::new(16, 4), Point::new(16, 22))
        .into_styled(fill)
        .draw(&mut fb)?;

    if volume.muted {
        Line::new(Point::new(20, 8), Point::new(28, 18))
            .into_styled(thick)
            .draw(&mut fb)?;
        Line::new(Point::new(28, 8), Point::new(20, 18))
            .into_styled(thick)
            .draw(&mut fb)?;
    } else {
        // Sound waves: more arcs at higher volume
        let waves = match volume.percent {
            0 => 0,
            1..=33 => 1,
            34..=66 => 2,
            _ => 3,
        };
        for i in 0..waves {
            Arc::with_center(Point::new(16, 13), 8 + i * 6, (-45.0_f32).deg(), 90.0_f32.deg())
                .into_styled(thin)
                .draw(&mut fb)?;
        }
    }

    // Percentage, right-aligned
    let label = if volume.muted {
        "MUTED".to_string()
    } else {
        format!("{}%", volume.percent)
    };
    let font = MonoTextStyle::new(&iso_8859_15::FONT_10X20, BinaryColor::On);
    let align = TextStyleBuilder::new()
        .alignment(Alignment::Right)
        .baseline(Baseline::Top)
        .build();
    Text::with_text_style(&label, Point::new(124, 3), font, align).draw(&mut fb)?;

    // Level bar (capped at 100% even if the volume is boosted above it)
    Rectangle::new(Point::new(4, 29), Size::new(120, 8))
        .into_styled(thin)
        .draw(&mut fb)?;
    let width = 116 * volume.percent.min(100) / 100;
    if width > 0 && !volume.muted {
        Rectangle::new(Point::new(6, 31), Size::new(width, 4))
            .into_styled(fill)
            .draw(&mut fb)?;
    }

    Ok(fb)
}
