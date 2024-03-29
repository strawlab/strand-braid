// Design for stm32f303re board:
// * PA6, PA7, PB0, PB1 reserved for optogenetics and backlighting LED control
//  (303re tim3 pwm ch1,ch2,ch3,ch4, nucleo64 CN10-13, CN10-15, CN7-34, CN10-24,
//  uno D12, D11, A3, n.a.)
// * PB4 optional extra IR LED to check tracking latency
// * PA2, PA3 used for serial comms (303re usart2, nucleo64 CN10-35, CN10-37, uno D1, n.a.)
// * PA5 user LED on board (303re gpioa5, nucleo64 CN10-11, uno D13)
// * Potential future enhancement: USB PA11, PA12 (CN10-14, CN10-12)

// TODO: check memory layout.
//   See https://stackoverflow.com/questions/44443619/how-to-write-read-to-flash-on-stm32f4-cortex-m4

#![no_main]
#![no_std]

use defmt_rtt as _; // global logger
use panic_probe as _;

use embedded_hal::PwmPin;

use embedded_hal::digital::v2::OutputPin;

use stm32f3xx_hal::{
    flash::FlashExt,
    gpio::{self, GpioExt, Output, PushPull, AF7},
    nb, pac,
    prelude::*,
    pwm::tim3,
    serial::{Event, Serial},
};

use embedded_time::rate::Hertz;

use defmt::{error, info};

use rtic::Mutex;

use led_box_comms::{ChannelState, DeviceState, FromDevice, OnState, ToDevice};
use stm32f3xx_hal::gpio::gpioa::PA5;

use json_lines::accumulator::{FeedResult, NewlinesAccumulator};

pub type UserLED = PA5<Output<PushPull>>;

const ZERO_INTENSITY: u16 = 0;
const LED_PWM_FREQ: Hertz = Hertz(500);

#[rtic::app(device = stm32f3xx_hal::pac, peripherals = true)]
mod app {
    use super::*;

    use heapless::spsc::{Consumer, Producer, Queue};
    type SerialType = Serial<pac::USART2, (gpio::PA2<AF7<PushPull>>, gpio::PA3<AF7<PushPull>>)>;

    const RX_BUF_SZ: usize = 512;
    const RX_Q_SZ: usize = 256;
    const TX_Q_SZ: usize = 128;

    // Late resources
    #[shared]
    struct Shared {
        serial: SerialType,
        green_led: UserLED,
    }

    #[local]
    struct Local {
        inner_led_state: InnerLedState,
        pwm3_ch1: stm32f3xx_hal::pwm::PwmChannel<
            stm32f3xx_hal::pwm::Tim3Ch1,
            stm32f3xx_hal::pwm::WithPins,
        >,
        pwm3_ch2: stm32f3xx_hal::pwm::PwmChannel<
            stm32f3xx_hal::pwm::Tim3Ch2,
            stm32f3xx_hal::pwm::WithPins,
        >,
        pwm3_ch3: stm32f3xx_hal::pwm::PwmChannel<
            stm32f3xx_hal::pwm::Tim3Ch3,
            stm32f3xx_hal::pwm::WithPins,
        >,
        pwm3_ch4: stm32f3xx_hal::pwm::PwmChannel<
            stm32f3xx_hal::pwm::Tim3Ch4,
            stm32f3xx_hal::pwm::WithPins,
        >,

        rx_prod: Producer<'static, u8, RX_Q_SZ>,
        rx_cons: Consumer<'static, u8, RX_Q_SZ>,

        tx_prod: Producer<'static, u8, TX_Q_SZ>,
        tx_cons: Consumer<'static, u8, TX_Q_SZ>,
    }

    #[init]
    fn init(c: init::Context) -> (Shared, Local, init::Monotonics) {
        // Device specific peripherals
        info!(
            "hello from f303, COMM_VERSION {}, BAUD_RATE {}, encoding {}",
            led_box_comms::COMM_VERSION,
            led_box_comms::BAUD_RATE,
            "JSON + newlines",
        );

        let mut flash = c.device.FLASH.constrain();
        let mut rcc = c.device.RCC.constrain();
        let mut gpioa = c.device.GPIOA.split(&mut rcc.ahb);
        let mut gpiob = c.device.GPIOB.split(&mut rcc.ahb);
        let clocks = rcc.cfgr.freeze(&mut flash.acr);

        // initialize serial
        let tx = gpioa
            .pa2
            .into_af_push_pull(&mut gpioa.moder, &mut gpioa.otyper, &mut gpioa.afrl);
        let rx = gpioa
            .pa3
            .into_af_push_pull(&mut gpioa.moder, &mut gpioa.otyper, &mut gpioa.afrl);

        let mut serial = Serial::new(
            c.device.USART2,
            (tx, rx),
            led_box_comms::BAUD_RATE.Bd(),
            clocks,
            &mut rcc.apb1,
        );
        serial.enable_interrupt(Event::ReceiveDataRegisterNotEmpty);

        {
            let pwm_freq_hz: Hertz = LED_PWM_FREQ.into();
            info!(
                "setting pwm to have resolution {}, freq {} Hz",
                led_box_comms::MAX_INTENSITY,
                pwm_freq_hz.0,
            );
        }

        // initialize pwm3
        info!("initializing tim3 for pwm");
        let tim3_channels = tim3(
            c.device.TIM3,
            led_box_comms::MAX_INTENSITY, // resolution of duty cycle
            LED_PWM_FREQ,                 // frequency of period
            &clocks,                      // To get the timer's clock speed
        );

        info!("initializing pwm pins");
        let pa6 = gpioa
            .pa6
            .into_af_push_pull(&mut gpioa.moder, &mut gpioa.otyper, &mut gpioa.afrl); // ch1

        let pa7 = gpioa
            .pa7
            .into_af_push_pull(&mut gpioa.moder, &mut gpioa.otyper, &mut gpioa.afrl); // ch2
        let pb0 = gpiob
            .pb0
            .into_af_push_pull(&mut gpiob.moder, &mut gpiob.otyper, &mut gpiob.afrl); // ch3
        let pb1 = gpiob
            .pb1
            .into_af_push_pull(&mut gpiob.moder, &mut gpiob.otyper, &mut gpiob.afrl); // ch4

        let mut pwm3_ch1 = tim3_channels.0.output_to_pa6(pa6);
        let mut pwm3_ch2 = tim3_channels.1.output_to_pa7(pa7);
        let mut pwm3_ch3 = tim3_channels.2.output_to_pb0(pb0);
        let mut pwm3_ch4 = tim3_channels.3.output_to_pb1(pb1);

        pwm3_ch1.set_duty(ZERO_INTENSITY);
        pwm3_ch2.set_duty(ZERO_INTENSITY);
        pwm3_ch3.set_duty(ZERO_INTENSITY);
        pwm3_ch4.set_duty(ZERO_INTENSITY);

        info!("all PWM channels set to 0%");

        pwm3_ch1.enable();
        pwm3_ch2.enable();
        pwm3_ch3.enable();
        pwm3_ch4.enable();

        let mut green_led = gpioa
            .pa5
            .into_push_pull_output(&mut gpioa.moder, &mut gpioa.otyper);
        green_led.set_low().unwrap();

        {
            use stm32f3xx_hal::gpio::gpiob::PB4;
            use stm32f3xx_hal::gpio::{Output, PushPull};

            let mut extra_ir_led: PB4<Output<PushPull>> = gpiob
                .pb4
                .into_push_pull_output(&mut gpiob.moder, &mut gpiob.otyper);

            extra_ir_led.set_high().unwrap();
        }

        let rx_queue: &'static mut Queue<u8, RX_Q_SZ> = {
            static mut Q: Queue<u8, RX_Q_SZ> = Queue::new();
            unsafe { &mut Q }
        };
        let (rx_prod, rx_cons) = rx_queue.split();

        let tx_queue: &'static mut Queue<u8, TX_Q_SZ> = {
            static mut Q: Queue<u8, TX_Q_SZ> = Queue::new();
            unsafe { &mut Q }
        };
        let (tx_prod, tx_cons) = tx_queue.split();

        // initialization of late resources
        (
            Shared { serial, green_led },
            Local {
                inner_led_state: InnerLedState::default(),
                pwm3_ch1,
                pwm3_ch2,
                pwm3_ch3,
                pwm3_ch4,
                rx_prod,
                rx_cons,
                tx_prod,
                tx_cons,
            },
            init::Monotonics(),
        )
    }

    #[idle(shared = [green_led, serial], local = [inner_led_state, pwm3_ch1, pwm3_ch2, pwm3_ch3, pwm3_ch4, rx_cons, tx_prod])]
    fn idle(mut ctx: idle::Context) -> ! {
        let mut decoder = NewlinesAccumulator::<RX_BUF_SZ>::new();
        let mut current_device_state = DeviceState::default();
        let mut out_buf = [0u8; 256];

        info!("starting idle loop");

        // ctx.shared
        //     .green_led
        //     .lock(|green_led| green_led.set_high().unwrap());

        loop {
            let ret = if let Some(ch) = ctx.local.rx_cons.dequeue() {
                let ret = match decoder.feed::<ToDevice>(&[ch]) {
                    FeedResult::Consumed => None,
                    FeedResult::OverFull(_remaining) => {
                        error!("frame overflow");
                        None
                    }
                    FeedResult::DeserError(_remaining) => {
                        error!("deserialization");
                        None
                    }
                    FeedResult::Success { data, remaining: _ } => Some(data),
                };
                ret
            } else {
                None
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
                for ch in encoded.iter() {
                    ctx.local.tx_prod.enqueue(*ch).unwrap();
                }

                defmt::trace!("idle pushed {} bytes", encoded.len());
                ctx.shared.serial.lock(|serial| {
                    serial.enable_interrupt(Event::TransmitDataRegisterEmtpy);
                });
            }
        }
    }

    #[task(binds = USART2_EXTI26, shared = [serial, green_led], local = [rx_prod, tx_cons])]
    fn protocol_serial_task(mut cx: protocol_serial_task::Context) {
        defmt::trace!("IRQ start");
        // cx.shared.green_led.lock(|green_led| {
        //     green_led.toggle().unwrap();
        // });

        cx.shared.serial.lock(|serial| {
            if serial.is_event_triggered(Event::ReceiveDataRegisterNotEmpty) {
                // got a byte
                defmt::trace!("IRQ ReceiveDataRegisterNotEmpty");
                match serial.read() {
                    // this will clear the ReceiveDataRegisterNotEmpty event
                    Ok(byte) => {
                        defmt::trace!("IRQ got byte: '{}'", byte as char);

                        cx.local.rx_prod.enqueue(byte).unwrap();
                        // serial.configure_interrupt(Event::TransmissionComplete, Toggle::On);
                    }
                    Err(nb::Error::WouldBlock) => {
                        //hmm?!
                        error!("nb::Error::WouldBlock but IRQ fired!?!?");
                    }
                    Err(nb::Error::Other(err)) => {
                        use stm32f3xx_hal::serial::Error;
                        match err {
                            Error::Framing => {
                                defmt::error!("IRQ serial Framing error");
                            }
                            Error::Noise => {
                                defmt::error!("IRQ serial Noise error");
                            }
                            Error::Overrun => {
                                defmt::error!("IRQ serial Overrun error");
                            }
                            Error::Parity => {
                                defmt::error!("IRQ serial Parity error");
                            }
                            _ => {
                                defmt::error!("IRQ serial error");
                            }
                        }
                    }
                };
            }

            // could send a byte
            if serial.is_event_triggered(Event::TransmitDataRegisterEmtpy) {
                defmt::trace!("IRQ TransmitDataRegisterEmtpy");
                match cx.local.tx_cons.dequeue() {
                    Some(ch) => {
                        defmt::trace!("IRQ got char from tx_cons: {}", ch as char);
                        serial.write(ch).unwrap(); // this will clear the TransmitDataRegisterEmtpy event
                    }
                    None => {
                        // nothing more to send
                        defmt::trace!("done sending");
                        serial.disable_interrupt(Event::TransmitDataRegisterEmtpy);
                    }
                };
            }
        });
    }

    fn update_led_state(next_state: &ChannelState, ctx: &mut idle::Context) {
        let set_pwm3_now;
        {
            let inner_led_state = &mut ctx.local.inner_led_state;

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
        }
        if let Some(pwm_period) = set_pwm3_now {
            defmt::debug!(
                "setting channel {} to period {}",
                next_state.num,
                pwm_period
            );
            match next_state.num {
                1 => ctx.local.pwm3_ch1.set_duty(pwm_period),
                2 => ctx.local.pwm3_ch2.set_duty(pwm_period),
                3 => ctx.local.pwm3_ch3.set_duty(pwm_period),
                4 => ctx.local.pwm3_ch4.set_duty(pwm_period),
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
