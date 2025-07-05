use clap::{Parser, command};
use std::{
    collections::HashMap,
    process::exit,
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicI32, Ordering},
    },
};
use windows::Win32::{
    Media::Audio::{
        AUDIO_VOLUME_NOTIFICATION_DATA,
        Endpoints::{
            IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl,
        },
        IMMDeviceEnumerator, MMDeviceEnumerator, eMultimedia, eRender,
    },
    System::Com::{
        CLSCTX_ALL, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
    },
};
use windows_core::implement;

static MUTED: AtomicBool = AtomicBool::new(false);
static VOL: AtomicI32 = AtomicI32::new(0);
static MUTE_ENDPOINT: OnceLock<String> = OnceLock::new();
static VOLCHANGE_ENDPOINT: OnceLock<String> = OnceLock::new();

#[allow(non_camel_case_types)]
#[implement(IAudioEndpointVolumeCallback)]
struct VolumeChangeCallback {}

impl IAudioEndpointVolumeCallback_Impl for VolumeChangeCallback_Impl {
    #[allow(non_snake_case)]
    fn OnNotify(
        &self,
        pnotify: *mut AUDIO_VOLUME_NOTIFICATION_DATA,
    ) -> ::windows::core::Result<()> {
        let rcl = reqwest::blocking::Client::new();
        let new_muted = unsafe { (*pnotify).bMuted.as_bool() };
        let new_vol = unsafe { ((*pnotify).fMasterVolume * 100.0) as i32 };
        log::debug!("callback triggered");
        if MUTED.load(Ordering::Relaxed) != new_muted {
            // we can only toggle
            match rcl.post(MUTE_ENDPOINT.get().unwrap()).send() {
                Ok(resp) => {
                    log::debug!("{} -> {}", MUTE_ENDPOINT.get().unwrap(), resp.status());
                }
                Err(e) => {
                    log::error!("error in trigger call: {:?}", e);
                }
            }
            MUTED.store(new_muted, Ordering::Relaxed);
        }
        if VOL.load(Ordering::Relaxed) != new_vol {
            let mut pl = HashMap::new();
            pl.insert("data", new_vol);
            match rcl.post(VOLCHANGE_ENDPOINT.get().unwrap()).json(&pl).send() {
                Ok(resp) => {
                    log::debug!("{} -> {}", VOLCHANGE_ENDPOINT.get().unwrap(), resp.status());
                }
                Err(e) => {
                    log::error!("error in trigger call: {:?}", e);
                }
            }
            VOL.store(new_vol, Ordering::Relaxed);
        }
        Ok(())
    }
}

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct LoudStalker {
    endpoint: String,

    #[arg(short, long, default_value = "volchange")]
    volchange_trigger: String,

    #[arg(short, long, default_value = "mute")]
    mute_trigger: String,

    #[arg(short, long)]
    debug: bool,
}

fn main() {
    let args = LoudStalker::parse();

    env_logger::Builder::new()
        .filter_level(if args.debug {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        })
        .init();

    VOLCHANGE_ENDPOINT
        .set(format!(
            "http://{}/interact/trigger/{}",
            args.endpoint, args.volchange_trigger,
        ))
        .unwrap();
    MUTE_ENDPOINT
        .set(format!(
            "http://{}/interact/trigger/{}",
            args.endpoint, args.mute_trigger
        ))
        .unwrap();
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).unwrap();
        log::debug!("initialized windows connection");

        let imm_device_enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER).unwrap_or_else(
                |err| {
                    eprintln!("ERROR: Couldn't get Media device enumerator: {err}");
                    exit(1);
                },
            );
        log::debug!("got media device enumerator");

        let default_device = imm_device_enumerator
            .GetDefaultAudioEndpoint(eRender, eMultimedia)
            .unwrap_or_else(|err| {
                eprintln!("ERROR: Couldn't get Default audio endpoint {err}");
                exit(1);
            });
        log::debug!("got default audio endpoint");

        let simple_audio_volume: IAudioEndpointVolume = default_device
            .Activate(CLSCTX_ALL, None)
            .unwrap_or_else(|err| {
                eprintln!("ERROR: Couldn't get Endpoint volume control: {err}");
                exit(1);
            });
        log::debug!("got endpoint volume control");

        let volume_callback: IAudioEndpointVolumeCallback = VolumeChangeCallback {}.into();
        simple_audio_volume
            .RegisterControlChangeNotify(&volume_callback)
            .unwrap_or_else(|err| {
                eprintln!("ERROR: Couldn't set volume change callback: {err}");
                exit(1);
            });
        log::debug!("set volume change callback");
        log::info!("volume stalking started");
        loop {}
    }
}
