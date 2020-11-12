use pylon_cxx_rs::HasProperties;

fn main() -> Result<(), pylon_cxx_rs::PylonError> {
    // Before using any pylon methods, the pylon runtime must be initialized.
    let _pylon = pylon_cxx_rs::PylonAutoInit::new();

    for device in pylon_cxx_rs::TlFactory::instance().enumerate_devices()? {
        println!("Device {} {} -------------", device.property_value("VendorName")?, device.property_value("SerialNumber")?);
        for name in  device.property_names()? {
            let value = device.property_value(&name)?;
            println!("  {}: {}", name, value);
        }
    }
    Ok(())
}
