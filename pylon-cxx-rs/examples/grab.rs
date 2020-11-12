const COUNT_IMAGES_TO_GRAB: u32 = 100;

fn main() -> Result<(), pylon_cxx_rs::PylonError> {
    // Before using any pylon methods, the pylon runtime must be initialized.
    let _pylon = pylon_cxx_rs::PylonAutoInit::new();

    // Create an instant camera object with the camera device found first.
    let camera = pylon_cxx_rs::TlFactory::instance().create_first_device()?;

    // Print the model name of the camera.
    println!("Using device {:?}", camera.device_info().model_name());

    // Start the grabbing of COUNT_IMAGES_TO_GRAB images.
    // The camera device is parameterized with a default configuration which
    // sets up free-running continuous acquisition.
    camera.start_grabbing(&pylon_cxx_rs::GrabOptions::default().count(COUNT_IMAGES_TO_GRAB))?;

    let mut grab_result = pylon_cxx_rs::GrabResult::new()?;

    // Camera.StopGrabbing() is called automatically by the RetrieveResult() method
    // when c_countOfImagesToGrab images have been retrieved.
    while camera.is_grabbing() {
        // Wait for an image and then retrieve it. A timeout of 5000 ms is used.
        camera.retrieve_result(
            5000,
            &mut grab_result,
            pylon_cxx_rs::TimeoutHandling::ThrowException,
        )?;

        // Image grabbed successfully?
        if grab_result.grab_succeeded()? {
            // Access the image data.
            println!("SizeX: {}", grab_result.width()?);
            println!("SizeY: {}", grab_result.height()?);

            let image_buffer = grab_result.buffer()?;
            println!("Gray value of first pixel: {}\n", image_buffer[0]);
        } else {
            println!(
                "Error: {} {}",
                grab_result.error_code()?,
                grab_result.error_description()?
            );
        }
    }

    Ok(())
}
