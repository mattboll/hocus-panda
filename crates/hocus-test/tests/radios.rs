//! The Wi-Fi networks and Bluetooth devices the settings list (#67),
//! through NetworkManager's and BlueZ's stand-ins on a private bus: joined
//! with a password or not, left, forgotten; discovered, paired with a
//! passkey confirmed or refused, connected, forgotten.

use std::sync::{Arc, Mutex};

use hocus_platform::{
    bluetooth::{Ask, Bluetooth, Bluez},
    wifi::{JoinError, NetworkManager, Security, Wifi},
};
use hocus_test::{
    bus::{PrivateBus, installed},
    devices::{FakeDevices, NETWORK, PASSKEY, WIFI_PASSWORD},
};

fn start() -> anyhow::Result<Option<(PrivateBus, FakeDevices)>> {
    if !installed("dbus-daemon") {
        eprintln!("skipped: no dbus-daemon");
        return Ok(None);
    }
    let bus = PrivateBus::start()?;
    let devices = FakeDevices::start(&bus.address)?;
    Ok(Some((bus, devices)))
}

fn networks(nm: &NetworkManager) -> anyhow::Result<Vec<hocus_platform::wifi::Network>> {
    match nm.read().map_err(anyhow::Error::msg)? {
        Wifi::Networks(list) => Ok(list),
        other => anyhow::bail!("no networks: {other:?}"),
    }
}

#[test]
fn networks_are_listed_joined_left_and_forgotten() -> anyhow::Result<()> {
    let Some((bus, devices)) = start()? else {
        return Ok(());
    };
    let nm = NetworkManager::connect(&Some(bus.address.clone())).map_err(anyhow::Error::msg)?;
    nm.scan();
    assert!(devices.asked().contains(&"RequestScan".to_owned()));

    // The joined one first, each name once, the hidden one left out.
    let list = networks(&nm)?;
    let names: Vec<&str> = list.iter().map(|n| n.ssid.as_str()).collect();
    assert_eq!(
        names,
        [NETWORK, "Panda den", "Office", "Café du Parc", "Tea house"]
    );
    assert!(list[0].connected && list[0].saved);
    assert_eq!(list[0].strength, 82);
    assert_eq!(list[1].security, Security::Personal);
    assert_eq!(list[2].security, Security::Other);
    assert_eq!(list[3].security, Security::Open);
    assert_eq!(list[4].security, Security::Wpa3);

    // A secured network asks for its password; a wrong one is not kept.
    assert_eq!(nm.join("Panda den", None), Err(JoinError::NeedsPassword));
    assert_eq!(nm.join("Panda den", Some("bamboo")), Err(JoinError::Failed));
    assert!(devices.asked().contains(&"Delete Panda den".to_owned()));
    assert!(
        !networks(&nm)?
            .iter()
            .any(|n| n.saved && n.ssid == "Panda den")
    );

    nm.join("Panda den", Some(WIFI_PASSWORD))
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let list = networks(&nm)?;
    assert_eq!(list[0].ssid, "Panda den");
    assert!(list[0].connected && list[0].saved);
    assert!(
        devices
            .asked()
            .contains(&"AddAndActivateConnection Panda den wpa-psk".to_owned())
    );
    assert!(
        !devices.asked().iter().any(|l| l.contains(WIFI_PASSWORD)),
        "the password is never written down"
    );

    // A saved one joins again without a password.
    nm.join(NETWORK, None)
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    assert!(
        devices
            .asked()
            .contains(&format!("ActivateConnection {NETWORK}"))
    );
    assert!(networks(&nm)?[0].connected);

    nm.disconnect().map_err(anyhow::Error::msg)?;
    assert!(!networks(&nm)?.iter().any(|n| n.connected));

    nm.forget("Panda den").map_err(anyhow::Error::msg)?;
    assert!(
        !networks(&nm)?
            .iter()
            .any(|n| n.ssid == "Panda den" && n.saved)
    );

    // An open network joins without one.
    nm.join("Café du Parc", None)
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    assert!(
        devices
            .asked()
            .contains(&"AddAndActivateConnection Café du Parc open".to_owned())
    );
    Ok(())
}

fn devices_of(bluez: &Bluez) -> anyhow::Result<Vec<hocus_platform::bluetooth::Device>> {
    match bluez.read().map_err(anyhow::Error::msg)? {
        Bluetooth::Devices(list) => Ok(list),
        other => anyhow::bail!("no devices: {other:?}"),
    }
}

#[test]
fn devices_are_discovered_paired_connected_and_forgotten() -> anyhow::Result<()> {
    let Some((bus, devices)) = start()? else {
        return Ok(());
    };
    let bluez = Bluez::connect(&Some(bus.address.clone())).map_err(anyhow::Error::msg)?;
    let names = |list: &[hocus_platform::bluetooth::Device]| {
        list.iter().map(|d| d.name.clone()).collect::<Vec<_>>()
    };
    let list = devices_of(&bluez)?;
    assert_eq!(names(&list), ["Bamboo headphones", "Panda mouse"]);
    assert_eq!(list[0].battery, Some(80));
    assert!(list[0].connected && !list[1].connected);

    // Found while discovering; the one without a name is not shown.
    bluez.discover(true).map_err(anyhow::Error::msg)?;
    let list = devices_of(&bluez)?;
    assert_eq!(
        names(&list),
        [
            "Bamboo headphones",
            "Panda mouse",
            "Panda keyboard",
            "Speaker by the window"
        ]
    );
    let keyboard = list[2].path.clone();
    let speaker = list[3].path.clone();

    // The keyboard shows a passkey: refused, it is not paired.
    let asked = Arc::new(Mutex::new(Vec::new()));
    let said = asked.clone();
    let refused = bluez.pair(&keyboard, move |ask| {
        if let Ok(mut said) = said.lock() {
            said.push(
                format!("{ask:?}")
                    .split(' ')
                    .next()
                    .unwrap_or("")
                    .to_owned(),
            );
        }
        if let Ask::Confirm { passkey, answer } = ask {
            assert_eq!(passkey, PASSKEY);
            answer.give(false);
        }
    });
    assert!(refused.is_err());
    assert!(
        devices
            .asked()
            .contains(&"Pair Panda keyboard rejected".to_owned())
    );
    assert!(!devices_of(&bluez)?[2].paired);

    // Confirmed: paired, trusted, connected.
    bluez
        .pair(&keyboard, |ask| {
            if let Ask::Confirm { answer, .. } = ask {
                answer.give(true);
            }
        })
        .map_err(anyhow::Error::msg)?;
    let asked = devices.asked();
    for line in [
        "RegisterAgent DisplayYesNo",
        "UnregisterAgent",
        "Trusted Panda keyboard true",
        "Connect Panda keyboard",
    ] {
        assert!(asked.contains(&line.to_owned()), "{line}: {asked:?}");
    }
    let keyboard_now = devices_of(&bluez)?
        .into_iter()
        .find(|d| d.path == keyboard)
        .ok_or_else(|| anyhow::anyhow!("the keyboard went"))?;
    assert!(keyboard_now.paired && keyboard_now.connected);

    // "Just works": nothing asked.
    bluez
        .pair(&speaker, |ask| {
            assert!(matches!(ask, Ask::Over), "asked {ask:?}");
        })
        .map_err(anyhow::Error::msg)?;

    bluez
        .connect_device(&keyboard, false)
        .map_err(anyhow::Error::msg)?;
    assert!(
        devices
            .asked()
            .contains(&"Disconnect Panda keyboard".to_owned())
    );
    bluez.forget(&keyboard).map_err(anyhow::Error::msg)?;
    assert!(!names(&devices_of(&bluez)?).contains(&"Panda keyboard".to_owned()));
    bluez.discover(false).map_err(anyhow::Error::msg)?;
    assert!(devices.asked().contains(&"StopDiscovery".to_owned()));
    Ok(())
}
