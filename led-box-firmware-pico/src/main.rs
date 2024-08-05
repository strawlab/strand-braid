// Design for RPi Pico:
// Pins in use:
//  PWM LED channel 1: gpio6
//  PWM LED channel 2: gpio7
//  PWM LED channel 3: gpio8
//  PWM LED channel 4: gpio9

// Reserved Pins (do not use except for these assignments):
//  Pico (but not Pico W) LED: gpio25
//  I2C0 SDA: gpio4
//  I2C0 SCL: gpio5
//  Pico W wireless: gpio23
//  Pico W wireless: gpio24
//  Pico W wireless: gpio25
//  Pico W wireless: gpio29

#![no_std]
#![no_main]

use defmt_rtt as _;
use panic_probe as _;
use rtic::Mutex;

use led_box_comms::{ChannelState, DeviceState, FromDevice, OnState, ToDevice};

use json_lines::accumulator::{FeedResult, NewlinesAccumulator};

const ZERO_INTENSITY: u16 = 0;
const LED_PWM_FREQ_HZ: f64 = 500.0;

#[rtic::app(device = rp_pico::hal::pac, peripherals = true, dispatchers = [I2C0_IRQ])]
mod app {
    use super::*;
    use rp_pico::XOSC_CRYSTAL_FREQ;

    use heapless::spsc::{Consumer, Producer, Queue};
    use usb_device::{class_prelude::*, prelude::*};
    use usbd_serial::SerialPort;

    use embedded_hal::{digital::v2::OutputPin, PwmPin};
    use rp2040_hal::{
        self as hal,
        clocks::init_clocks_and_plls,
        pwm,
        timer::{monotonic::Monotonic, Alarm0},
        usb::UsbBus,
        watchdog::Watchdog,
        Sio,
    };

    pub struct PwmData {
        pwm3_slice: pwm::Slice<pwm::Pwm3, pwm::FreeRunning>,
        pwm4_slice: pwm::Slice<pwm::Pwm4, pwm::FreeRunning>,
        // pwm_clock_tick_period: f32,
    }

    const MAX_FRAME_SZ: usize = 256;
    const NUM_FRAMES: usize = 8;
    type UsbFrame = heapless::Vec<u8, MAX_FRAME_SZ>;

    #[shared]
    struct Shared {
        green_led: hal::gpio::Pin<
            hal::gpio::bank0::Gpio25,
            hal::gpio::FunctionSioOutput,
            hal::gpio::PullNone,
        >,
        inner_led_state: InnerLedState,
        usb_serial: SerialPort<'static, UsbBus>,
    }

    #[monotonic(binds = TIMER_IRQ_0, default = true)]
    type MyMono = Monotonic<Alarm0>;

    #[local]
    struct Local {
        pwms: PwmData,
        usb_dev: UsbDevice<'static, UsbBus>,
        rx_prod: Producer<'static, UsbFrame, NUM_FRAMES>,
        rx_cons: Consumer<'static, UsbFrame, NUM_FRAMES>,
    }

    #[init(local = [usb_bus: Option<UsbBusAllocator<UsbBus>> = None])]
    fn init(c: init::Context) -> (Shared, Local, init::Monotonics) {
        let divider = 10;
        let top: f64 = 125.0e6 / (f64::try_from(divider).unwrap()) / LED_PWM_FREQ_HZ;
        assert!(top <= u16::MAX as f64);
        let top: u16 = top as u16;
        // let myduty = mytop / 2;
        // mypwm.default_config();
        // mypwm.set_div_int(divider);
        // mypwm.set_top(mytop);
        // mypwm.channel_a.set_duty(myduty);
        // mypwm.channel_a.output_to(pins.gpio8);

        // let divider = 20;
        // let top = 0xffff;
        let pwm_freq = 125e6 / (top as f64 * divider as f64); // 95.4 Hz
        defmt::info!(
            "Hello from {}. (COMM_VERSION: {}, pwm_freq: {}, encoding {})",
            env!["CARGO_PKG_NAME"],
            led_box_comms::COMM_VERSION,
            pwm_freq,
            "JSON + newlines",
        );
        let mut resets = c.device.RESETS;
        let mut watchdog = Watchdog::new(c.device.WATCHDOG);
        let clocks = init_clocks_and_plls(
            XOSC_CRYSTAL_FREQ,
            c.device.XOSC,
            c.device.CLOCKS,
            c.device.PLL_SYS,
            c.device.PLL_USB,
            &mut resets,
            &mut watchdog,
        )
        .ok()
        .unwrap();
        let mut timer = hal::Timer::new(c.device.TIMER, &mut resets, &clocks);

        let usb_bus = c.local.usb_bus;
        usb_bus.replace(UsbBusAllocator::new(UsbBus::new(
            c.device.USBCTRL_REGS,
            c.device.USBCTRL_DPRAM,
            clocks.usb_clock,
            true,
            &mut resets,
        )));
        let usb_serial = SerialPort::new(usb_bus.as_ref().unwrap());

        let usb_dev = UsbDeviceBuilder::new(usb_bus.as_ref().unwrap(), UsbVidPid(0x16c0, 0x27dd))
            .manufacturer("Straw Lab")
            .product("LED Box")
            .serial_number("TEST")
            .device_class(2) // USB_CLASS_CDC
            .build();

        let sio = Sio::new(c.device.SIO);
        let pins = rp_pico::Pins::new(
            c.device.IO_BANK0,
            c.device.PADS_BANK0,
            sio.gpio_bank0,
            &mut resets,
        );

        let mut green_led = pins.led.reconfigure();
        green_led.set_low().unwrap();

        //set up PWM
        let pwm_slices = pwm::Slices::new(c.device.PWM, &mut resets);

        let mut pwm3_slice = pwm_slices.pwm3;
        pwm3_slice.default_config();
        pwm3_slice.set_div_int(divider as u8);
        pwm3_slice.set_top(top);

        let mut pwm4_slice = pwm_slices.pwm4;
        pwm4_slice.default_config();
        pwm4_slice.set_div_int(divider as u8);
        pwm4_slice.set_top(top);

        let mut pwms = PwmData {
            pwm3_slice,
            pwm4_slice,
            // pwm_clock_tick_period: divider as f32 / 125e6,
        };
        defmt::info!("set PWMS initially!");
        // set_pwms(&mut pwms, DeviceState::default());
        pwms.pwm3_slice.channel_a.output_to(pins.gpio6);
        pwms.pwm3_slice.channel_b.output_to(pins.gpio7);
        pwms.pwm3_slice.enable();

        pwms.pwm4_slice.channel_a.output_to(pins.gpio8);
        pwms.pwm4_slice.channel_b.output_to(pins.gpio9);
        pwms.pwm4_slice.enable();

        let rx_queue: &'static mut Queue<UsbFrame, NUM_FRAMES> = {
            static mut Q: Queue<UsbFrame, NUM_FRAMES> = Queue::new();
            unsafe { core::ptr::addr_of_mut!(Q).as_mut().unwrap() }
        };
        let (rx_prod, rx_cons) = rx_queue.split();

        let alarm = timer.alarm_0().unwrap();
        // blink_led::spawn_after(500.millis()).unwrap();

        (
            Shared {
                green_led,
                usb_serial,
                inner_led_state: InnerLedState::default(),
            },
            Local {
                pwms,
                usb_dev,
                rx_prod,
                rx_cons,
            },
            init::Monotonics(Monotonic::new(timer, alarm)),
        )
    }

    #[idle(shared = [usb_serial, inner_led_state, green_led], local = [pwms,rx_cons])]
    fn idle(mut ctx: idle::Context) -> ! {
        let mut decoder = NewlinesAccumulator::<512>::new();
        let mut current_device_state = DeviceState::default();
        let mut out_buf = [0u8; 256];

        loop {
            let frame = match ctx.local.rx_cons.dequeue() {
                Some(frame) => frame,
                None => continue,
            };
            let src = &frame.as_slice();

            let ret = match decoder.feed::<ToDevice>(src) {
                FeedResult::Consumed => None,
                FeedResult::OverFull(_remaining) => {
                    defmt::error!("frame overflow");
                    None
                }
                FeedResult::DeserError(_remaining) => {
                    defmt::error!("deserialization");
                    None
                }
                FeedResult::Success { data, remaining: _ } => Some(data),
            };

            if let Some(msg) = ret {
                let response;
                match msg {
                    ToDevice::DeviceState(next_state) => {
                        update_device_state(&mut current_device_state, &next_state, &mut ctx);
                        response = FromDevice::StateWasSet;
                        defmt::debug!("device state set");
                    }
                    ToDevice::EchoRequest8(buf) => {
                        response = FromDevice::EchoResponse8(buf);
                        defmt::debug!("echo");
                    }
                    ToDevice::VersionRequest => {
                        response = FromDevice::VersionResponse(led_box_comms::COMM_VERSION);
                        defmt::debug!("version request");
                    }
                }

                let encoded = json_lines::to_slice_newline(&response, &mut out_buf[..]).unwrap();

                ctx.shared.usb_serial.lock(|usb_serial| {
                    usb_serial.write(&encoded).unwrap();
                });
                defmt::trace!("sent {} bytes", encoded.len());
            }
        }
    }

    /// This function is called from the USB interrupt handler function (which
    /// does not have a return value). By here returning Result, we can abort
    /// processing early using idiomatic rust, even in the interrupt handler
    /// function.
    #[inline]
    fn on_usb_inner(
        usb_serial: &mut SerialPort<'static, UsbBus>,
        rx_prod: &mut Producer<'static, UsbFrame, NUM_FRAMES>,
    ) -> Result<usize, ()> {
        let mut new_frame = UsbFrame::new();
        new_frame.resize_default(MAX_FRAME_SZ)?;
        let new_frame_data = new_frame.as_mut_slice();

        match usb_serial.read(&mut new_frame_data[..]) {
            Ok(sz) => {
                new_frame.resize_default(sz)?;
                rx_prod.enqueue(new_frame).map_err(|_e| ())?;
                Ok(sz)
            }
            Err(usb_device::UsbError::WouldBlock) => Ok(0),
            Err(e) => {
                // Maybe the error is recoverable and we should not panic?
                panic!("usb error: {:?}", e);
            }
        }
    }

    #[task(binds=USBCTRL_IRQ, shared = [usb_serial], local=[usb_dev, rx_prod])]
    fn on_usb(ctx: on_usb::Context) {
        let mut usb_serial = ctx.shared.usb_serial;
        let usb_dev = ctx.local.usb_dev;
        let rx_prod = ctx.local.rx_prod;
        usb_serial.lock(|usb_serial| {
            if !usb_dev.poll(&mut [&mut *usb_serial]) {
                return;
            }
            match on_usb_inner(usb_serial, rx_prod) {
                Ok(0) => {}
                Ok(nbytes) => {
                    defmt::trace!("received {} bytes", nbytes);
                }
                Err(_) => {
                    defmt::error!("USB error");
                }
            }
        })
    }

    fn update_led_state(next_state: &ChannelState, ctx: &mut idle::Context) {
        let mut set_pwm3_now = None;
        {
            ctx.shared.inner_led_state.lock(|inner_led_state| {
                // borrowck scope for mutable reference into current_state
                let inner_led_chan_state: &mut InnerLedChannelState = match next_state.num {
                    1 => &mut inner_led_state.ch1,
                    2 => &mut inner_led_state.ch2,
                    3 => &mut inner_led_state.ch3,
                    4 => &mut inner_led_state.ch4,
                    _ => panic!("unknown channel"),
                };

                // we can assume inner_led_chan_state corresponds to correct channel in next_state
                // and thus we should ignore next_state.channel here

                // Calculate pwm period required for desired intensity.
                let pwm_period = match next_state.on_state {
                    OnState::Off => ZERO_INTENSITY,
                    OnState::ConstantOn => next_state.intensity,
                };

                if next_state.num == 1 {
                    match next_state.on_state {
                        OnState::Off => ctx
                            .shared
                            .green_led
                            .lock(|green_led| green_led.set_low().unwrap()),
                        OnState::ConstantOn => ctx
                            .shared
                            .green_led
                            .lock(|green_led| green_led.set_high().unwrap()),
                    };
                }

                // Based on on_state, decide what to do.
                match next_state.on_state {
                    OnState::Off | OnState::ConstantOn => {
                        set_pwm3_now = Some(pwm_period);
                        inner_led_chan_state.period = pwm_period;
                    }
                }
            })
        }
        if let Some(pwm_period) = set_pwm3_now {
            defmt::info!(
                "setting channel {} to period {}",
                next_state.num,
                pwm_period
            );
            match next_state.num {
                1 => ctx.local.pwms.pwm3_slice.channel_a.set_duty(pwm_period),
                2 => ctx.local.pwms.pwm3_slice.channel_b.set_duty(pwm_period),
                3 => ctx.local.pwms.pwm4_slice.channel_a.set_duty(pwm_period),
                4 => ctx.local.pwms.pwm4_slice.channel_b.set_duty(pwm_period),
                _ => panic!("unknown channel"),
            };
        }
        // rtic::pend(pac::Interrupt::TIM2);
    }

    fn update_device_state(
        current_state: &mut DeviceState,
        next_state: &DeviceState,
        mut ctx: &mut idle::Context,
    ) {
        if current_state.ch1 != next_state.ch1 {
            update_led_state(&next_state.ch1, &mut ctx);
            current_state.ch1 = next_state.ch1;
        }
        if current_state.ch2 != next_state.ch2 {
            update_led_state(&next_state.ch2, &mut ctx);
            current_state.ch2 = next_state.ch2;
        }
        if current_state.ch3 != next_state.ch3 {
            update_led_state(&next_state.ch3, &mut ctx);
            current_state.ch3 = next_state.ch3;
        }
        if current_state.ch4 != next_state.ch4 {
            update_led_state(&next_state.ch4, &mut ctx);
            current_state.ch4 = next_state.ch4;
        }
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum MyChan {
    Ch1,
    Ch2,
    Ch3,
    Ch4,
}

/// This keeps track of our actual low-level state
#[derive(Debug, PartialEq, Clone, Copy)]
struct InnerLedChannelState {
    tim3_channel: MyChan,
    period: u16,
}

impl InnerLedChannelState {
    const fn default(tim3_channel: MyChan) -> Self {
        Self {
            tim3_channel,
            period: 0,
        }
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct InnerLedState {
    ch1: InnerLedChannelState,
    ch2: InnerLedChannelState,
    ch3: InnerLedChannelState,
    ch4: InnerLedChannelState,
}

impl InnerLedState {
    const fn default() -> Self {
        Self {
            ch1: InnerLedChannelState::default(MyChan::Ch1),
            ch2: InnerLedChannelState::default(MyChan::Ch2),
            ch3: InnerLedChannelState::default(MyChan::Ch3),
            ch4: InnerLedChannelState::default(MyChan::Ch4),
        }
    }
}
