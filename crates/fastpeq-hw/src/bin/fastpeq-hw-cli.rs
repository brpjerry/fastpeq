//! Driver-development CLI for `fastpeq-hw`.
//!
//! Exposes every entry point the fastpeq app uses to talk to hardware-EQ
//! devices — enumeration ([`fastpeq_hw::detect`]), output-name correlation
//! ([`fastpeq_hw::device_for_output`]), capability lookup ([`fastpeq_hw::profile`]),
//! direct device I/O ([`fastpeq_hw::HardwareEq`] via [`fastpeq_hw::open`]), and the
//! rate-limited worker path ([`fastpeq_hw::HardwareSession`], what the GUI actually
//! drives) — plus the raw HID layer for bringing up a device that has no driver
//! yet. If a new device works through every command here, it works in fastpeq.
//!
//! Run `fastpeq-hw-cli help` (or `cargo run -p fastpeq-hw -- help`) for usage.

use fastpeq_core::{HwBand, HwFilterType};
use fastpeq_hw::{DetectedDevice, HardwareSession};
use std::process::ExitCode;

const USAGE: &str = "\
fastpeq-hw-cli — drive fastpeq's hardware-EQ layer without the GUI

USAGE: fastpeq-hw-cli [--dry-run] <COMMAND> [ARGS]

App-surface commands (what fastpeq calls):
  list [--json]                     Detect supported devices (fastpeq_hw::detect)
  match <output-name>               Resolve an audio output's friendly name to a
                                    supported device (fastpeq_hw::device_for_output)
  profile <device>                  A device's PEQ capabilities, without opening it
  version <device>                  Open the device and read its firmware version
  pull <device>                     Read the bands currently on the device
  push <device> [--pregain <dB>] [--commit] [BAND...]
                                    Write bands directly (HardwareEq::push).
                                    No bands = flatten. --commit also saves to flash.
  session <device>                  Interactive worker session (HardwareSession) —
                                    the coalesced/throttled path the GUI uses.

Bring-up commands (for devices with no driver yet):
  enumerate                         List every HID interface/collection present
  raw <iface> <OP>...               Raw report I/O on one HID interface. Ops, in order:
                                      send <hex>         write an output report
                                                         (first byte = report id)
                                      read [<ms>]        read one input report
                                                         (default timeout 1000 ms)
                                      sendf <hex>        send a feature report
                                      readf <id> [<len>] get a feature report
                                                         (default len 64)

Selectors:
  <device>   an index from `list`, a model/name substring (e.g. KA17), or a HID path
  <iface>    an index from `enumerate`, vid:pid[:usage_page] in hex
             (e.g. 2e3c:5310 or 2e3c:5310:ff00), or a HID path

BAND syntax: <type>:<freq>:<gain>:<q> with type pk|ls|hs, e.g. pk:1000:-3.5:1.4

--dry-run (or FASTPEQ_HW_DRYRUN=1) makes drivers log packets instead of writing —
safe first contact with an unverified protocol. Raw ops ignore it and always touch
the device.
";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(pos) = args.iter().position(|a| a == "--dry-run") {
        args.remove(pos);
        // Safe: nothing else is running yet — set before any worker thread spawns.
        unsafe { std::env::set_var("FASTPEQ_HW_DRYRUN", "1") };
    }
    let Some(cmd) = args.first().cloned() else {
        eprint!("{USAGE}");
        return ExitCode::FAILURE;
    };
    let rest = &args[1..];
    let result = match cmd.as_str() {
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            Ok(())
        }
        "list" => cmd_list(rest),
        "match" => cmd_match(rest),
        "profile" => cmd_profile(rest),
        "version" => cmd_version(rest),
        "pull" => cmd_pull(rest),
        "push" => cmd_push(rest),
        "session" => cmd_session(rest),
        "enumerate" => cmd_enumerate(),
        "raw" => cmd_raw(rest),
        other => Err(format!("unknown command `{other}` — try `help`")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Resolve a `<device>` selector against `detect()`: an index from `list`, a
/// case-insensitive model/name substring, or a full HID path.
fn resolve_device(sel: &str) -> Result<DetectedDevice, String> {
    let devices = fastpeq_hw::detect()?;
    if let Some(d) = devices.iter().find(|d| d.id == sel) {
        return Ok(d.clone());
    }
    if let Ok(i) = sel.parse::<usize>() {
        return devices
            .get(i)
            .cloned()
            .ok_or_else(|| format!("no device at index {i} — `list` shows {}", devices.len()));
    }
    let needle = sel.to_uppercase();
    let matches: Vec<&DetectedDevice> = devices
        .iter()
        .filter(|d| d.name.to_uppercase().contains(&needle))
        .collect();
    match matches.as_slice() {
        [d] => Ok((*d).clone()),
        [] => Err(format!(
            "no detected device matches `{sel}` ({} detected — see `list`)",
            devices.len()
        )),
        many => Err(format!(
            "`{sel}` is ambiguous: {}",
            many.iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn arg<'a>(args: &'a [String], what: &str) -> Result<&'a str, String> {
    args.first()
        .map(String::as_str)
        .ok_or_else(|| format!("missing {what} argument — try `help`"))
}

/// Parse `<type>:<freq>:<gain>:<q>` (e.g. `pk:1000:-3.5:1.4`).
fn parse_band(s: &str) -> Result<HwBand, String> {
    let parts: Vec<&str> = s.split(':').collect();
    let [kind, freq, gain, q] = parts.as_slice() else {
        return Err(format!(
            "bad band `{s}` — expected <type>:<freq>:<gain>:<q>"
        ));
    };
    let kind = match kind.to_lowercase().as_str() {
        "pk" | "peak" => HwFilterType::Peak,
        "ls" | "lowshelf" => HwFilterType::LowShelf,
        "hs" | "highshelf" => HwFilterType::HighShelf,
        other => return Err(format!("bad filter type `{other}` — use pk, ls or hs")),
    };
    let num = |v: &str, what: &str| {
        v.parse::<f64>()
            .map_err(|_| format!("bad {what} `{v}` in band `{s}`"))
    };
    Ok(HwBand {
        kind,
        freq: num(freq, "frequency")?,
        gain: num(gain, "gain")?,
        q: num(q, "Q")?,
    })
}

fn band_str(b: &HwBand) -> String {
    let kind = match b.kind {
        HwFilterType::Peak => "pk",
        HwFilterType::LowShelf => "ls",
        HwFilterType::HighShelf => "hs",
    };
    format!(
        "{kind}  {:>8.1} Hz  {:>+6.2} dB  Q {:.2}",
        b.freq, b.gain, b.q
    )
}

fn print_device(i: usize, d: &DetectedDevice) {
    println!("[{i}] {}", d.name);
    println!("    model: {}  manufacturer: {}", d.model, d.manufacturer);
    println!(
        "    max_filters: {}  user_pregain: {}  commit_to_apply: {}  commit_delay_ms: {}",
        d.max_filters, d.user_pregain, d.commit_to_apply, d.commit_delay_ms
    );
    println!("    id: {}", d.id);
}

fn cmd_list(args: &[String]) -> Result<(), String> {
    let devices = fastpeq_hw::detect()?;
    if args.iter().any(|a| a == "--json") {
        println!(
            "{}",
            serde_json::to_string_pretty(&devices).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    if devices.is_empty() {
        println!("no supported devices detected (`enumerate` lists all HID interfaces)");
    }
    for (i, d) in devices.iter().enumerate() {
        print_device(i, d);
    }
    Ok(())
}

fn cmd_match(args: &[String]) -> Result<(), String> {
    let name = arg(args, "<output-name>")?;
    match fastpeq_hw::device_for_output(name) {
        Some(d) => {
            println!("output `{name}` resolves to a supported device — fastpeq would offload:");
            print_device(0, &d);
        }
        None => println!("output `{name}` matches no supported device — fastpeq would not offload"),
    }
    Ok(())
}

fn cmd_profile(args: &[String]) -> Result<(), String> {
    let dev = resolve_device(arg(args, "<device>")?)?;
    let p = fastpeq_hw::profile(&dev.id)?;
    println!("{}", dev.name);
    println!("  max_filters:        {}", p.max_filters);
    println!("  sample_rate:        {} Hz", p.sample_rate);
    println!("  gain_range:         {:?} dB", p.gain_range);
    println!("  q_range:            {:?}", p.q_range);
    println!("  freq_range:         {:?} Hz", p.freq_range);
    println!("  supports_low_shelf: {}", p.supports_low_shelf);
    println!("  supports_high_shelf:{}", p.supports_high_shelf);
    println!("  user_pregain:       {}", p.user_pregain);
    println!("  commit_to_apply:    {}", p.commit_to_apply);
    println!("  commit_delay_ms:    {}", p.commit_delay_ms);
    Ok(())
}

fn cmd_version(args: &[String]) -> Result<(), String> {
    let dev = resolve_device(arg(args, "<device>")?)?;
    let mut hw = fastpeq_hw::open(&dev.id)?;
    println!("{}: firmware {}", dev.name, hw.version()?);
    Ok(())
}

fn cmd_pull(args: &[String]) -> Result<(), String> {
    let dev = resolve_device(arg(args, "<device>")?)?;
    let mut hw = fastpeq_hw::open(&dev.id)?;
    let bands = hw.pull()?;
    if bands.is_empty() {
        println!(
            "{}: no bands read back (firmware may not answer reads)",
            dev.name
        );
    }
    for (i, b) in bands.iter().enumerate() {
        println!("[{i}] {}", band_str(b));
    }
    Ok(())
}

fn cmd_push(args: &[String]) -> Result<(), String> {
    let dev = resolve_device(arg(args, "<device>")?)?;
    let mut pregain = 0.0;
    let mut commit = false;
    let mut bands = Vec::new();
    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--commit" => commit = true,
            "--pregain" => {
                let v = it.next().ok_or("--pregain needs a value (dB)")?;
                pregain = v
                    .parse::<f64>()
                    .map_err(|_| format!("bad --pregain value `{v}`"))?;
            }
            band => bands.push(parse_band(band)?),
        }
    }
    if bands.len() > dev.max_filters {
        return Err(format!(
            "{} bands exceed the device's budget of {}",
            bands.len(),
            dev.max_filters
        ));
    }
    let mut hw = fastpeq_hw::open(&dev.id)?;
    hw.push(&bands, pregain, commit)?;
    println!(
        "pushed {} band(s), pregain {pregain} dB{} to {}",
        bands.len(),
        if commit {
            ", committed to flash"
        } else {
            " (volatile)"
        },
        dev.name
    );
    Ok(())
}

/// Interactive worker session — the exact path the GUI drives: coalesced,
/// throttled pushes on a dedicated thread that owns the device handle.
fn cmd_session(args: &[String]) -> Result<(), String> {
    use std::io::{BufRead, Write};

    let dev = resolve_device(arg(args, "<device>")?)?;
    let profile = fastpeq_hw::profile(&dev.id)?;
    let session = HardwareSession::start(dev, profile);

    // Wait briefly for the worker's open + version handshake, like the app's
    // "connecting to hardware" hint.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        let s = session.status();
        if s.connected || s.error.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    print_status(&session);

    println!("commands: push <pregain> [BAND...] | commit <pregain> [BAND...] | status | quit");
    let stdin = std::io::stdin();
    loop {
        print!("> ");
        std::io::stdout().flush().ok();
        let mut line = String::new();
        if stdin
            .lock()
            .read_line(&mut line)
            .map_err(|e| e.to_string())?
            == 0
        {
            break; // EOF
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        let outcome = match words.as_slice() {
            [] => Ok(()),
            ["quit"] | ["exit"] => break,
            ["status"] => {
                print_status(&session);
                Ok(())
            }
            [cmd @ ("push" | "commit"), pregain, bands @ ..] => (|| {
                let pregain = pregain
                    .parse::<f64>()
                    .map_err(|_| format!("bad pregain `{pregain}`"))?;
                let bands = bands
                    .iter()
                    .map(|b| parse_band(b))
                    .collect::<Result<Vec<_>, _>>()?;
                if *cmd == "push" {
                    session.push_live(bands, pregain)
                } else {
                    session.push_commit(bands, pregain)
                }
            })(),
            _ => Err("unrecognized — commands: push, commit, status, quit".to_string()),
        };
        if let Err(e) = outcome {
            eprintln!("error: {e}");
        }
    }
    session.stop();
    println!("session closed, device released");
    Ok(())
}

fn print_status(session: &HardwareSession) {
    let s = session.status();
    println!(
        "{}: connected={}  firmware={}  error={}",
        session.descriptor.name,
        s.connected,
        s.version.as_deref().unwrap_or("?"),
        s.error.as_deref().unwrap_or("none"),
    );
}

// ---------------------------------------------------------------------------
// Bring-up commands: the raw HID layer (Windows only, like the drivers).

#[cfg(windows)]
fn cmd_enumerate() -> Result<(), String> {
    let infos = fastpeq_hw::hid::enumerate()?;
    for (i, d) in infos.iter().enumerate() {
        println!(
            "[{i}] vid:pid {:04x}:{:04x}  usage_page 0x{:04x}  {} {}",
            d.vendor_id,
            d.product_id,
            d.usage_page,
            d.manufacturer.trim(),
            d.product.trim(),
        );
        println!("     path: {}", d.path);
    }
    println!("{} HID interface(s)", infos.len());
    Ok(())
}

/// Resolve an `<iface>` selector against the raw enumeration: an index from
/// `enumerate`, `vid:pid[:usage_page]` in hex, or a full HID path.
#[cfg(windows)]
fn resolve_iface(sel: &str) -> Result<String, String> {
    let infos = fastpeq_hw::hid::enumerate()?;
    if infos.iter().any(|d| d.path == sel) {
        return Ok(sel.to_string());
    }
    if let Ok(i) = sel.parse::<usize>() {
        return infos.get(i).map(|d| d.path.clone()).ok_or_else(|| {
            format!(
                "no interface at index {i} — `enumerate` shows {}",
                infos.len()
            )
        });
    }
    let parts: Vec<&str> = sel.split(':').collect();
    if let [vid, pid, rest @ ..] = parts.as_slice()
        && rest.len() <= 1
        && let (Ok(vid), Ok(pid)) = (u16::from_str_radix(vid, 16), u16::from_str_radix(pid, 16))
    {
        let usage = match rest.first() {
            Some(u) => {
                Some(u16::from_str_radix(u, 16).map_err(|_| format!("bad usage page `{u}`"))?)
            }
            None => None,
        };
        let matches: Vec<_> = infos
            .iter()
            .filter(|d| {
                d.vendor_id == vid && d.product_id == pid && usage.is_none_or(|u| d.usage_page == u)
            })
            .collect();
        return match matches.as_slice() {
            [d] => Ok(d.path.clone()),
            [] => Err(format!("no HID interface matches `{sel}`")),
            many => Err(format!(
                "`{sel}` matches {} interfaces (usage pages {}) — add :<usage_page> or use the index",
                many.len(),
                many.iter()
                    .map(|d| format!("0x{:04x}", d.usage_page))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        };
    }
    Err(format!(
        "can't resolve interface `{sel}` — use an index, vid:pid[:usage_page], or a path"
    ))
}

/// Strip separators and parse hex bytes, e.g. `4b 01 09` / `4b:01:09` / `0x4b0109`.
#[cfg(windows)]
fn parse_hex(s: &str) -> Result<Vec<u8>, String> {
    let clean: String = s
        .chars()
        .filter(|c| !matches!(c, ' ' | ':' | ',' | '-' | '_'))
        .collect();
    let clean = clean.strip_prefix("0x").unwrap_or(&clean);
    if !clean.len().is_multiple_of(2) {
        return Err(format!("odd number of hex digits in `{s}`"));
    }
    (0..clean.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).map_err(|_| format!("bad hex in `{s}`")))
        .collect()
}

#[cfg(windows)]
fn hex_dump(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(windows)]
fn cmd_raw(args: &[String]) -> Result<(), String> {
    let path = resolve_iface(arg(args, "<iface>")?)?;
    let dev = fastpeq_hw::hid::open(&path)?;
    let mut it = args[1..].iter();
    let mut ran_op = false;
    while let Some(op) = it.next() {
        ran_op = true;
        match op.as_str() {
            "send" => {
                let bytes = parse_hex(it.next().ok_or("send needs a hex payload")?)?;
                let n = dev.write(&bytes).map_err(|e| e.to_string())?;
                println!("sent {n} bytes: {}", hex_dump(&bytes));
            }
            "sendf" => {
                let bytes = parse_hex(it.next().ok_or("sendf needs a hex payload")?)?;
                dev.send_feature_report(&bytes).map_err(|e| e.to_string())?;
                println!("sent feature report: {}", hex_dump(&bytes));
            }
            "read" => {
                // A bare number after `read` is its timeout; anything else is the next op.
                let timeout = match it.clone().next().and_then(|a| a.parse::<i32>().ok()) {
                    Some(ms) => {
                        it.next();
                        ms
                    }
                    None => 1000,
                };
                let mut buf = [0u8; 256];
                let n = dev
                    .read_timeout(&mut buf, timeout)
                    .map_err(|e| e.to_string())?;
                if n == 0 {
                    println!("read: no report within {timeout} ms");
                } else {
                    println!("read {n} bytes: {}", hex_dump(&buf[..n]));
                }
            }
            "readf" => {
                let id = parse_hex(it.next().ok_or("readf needs a report id (hex)")?)?;
                let [id] = id.as_slice() else {
                    return Err("readf report id must be one byte".to_string());
                };
                let len = match it.clone().next().and_then(|a| a.parse::<usize>().ok()) {
                    Some(v) => {
                        it.next();
                        v
                    }
                    None => 64,
                };
                let mut buf = vec![0u8; len + 1];
                buf[0] = *id;
                let n = dev
                    .get_feature_report(&mut buf)
                    .map_err(|e| e.to_string())?;
                println!("feature report: {}", hex_dump(&buf[..n]));
            }
            other => {
                return Err(format!(
                    "unknown raw op `{other}` — send, sendf, read, readf"
                ));
            }
        }
    }
    if !ran_op {
        return Err(
            "no raw ops given — send <hex>, sendf <hex>, read [<ms>], readf <id> [<len>]"
                .to_string(),
        );
    }
    Ok(())
}

#[cfg(not(windows))]
fn cmd_enumerate() -> Result<(), String> {
    Err("raw HID access is only supported on Windows".to_string())
}

#[cfg(not(windows))]
fn cmd_raw(_args: &[String]) -> Result<(), String> {
    Err("raw HID access is only supported on Windows".to_string())
}
