// Design for stm32f303re board:
// * PA6, PA7, PB0, PB1 reserved for optogenetics and backlighting LED control
//  (303re tim3 pwm ch1,ch2,ch3,ch4, nucleo64 CN10-13, CN10-15, CN7-34, CN10-24,
//  uno D12, D11, A3, n.a.)
// * PB4 optional extra IR LED to check tracking latency
// * PA2, PA3 used for serial comms (303re usart2, nucleo64 CN10-35, CN10-37, uno D1, n.a.)
// * PA5 user LED on board (303re gpioa5, nucleo64 CN10-11, uno D13)
// * PA8 used as TIM1_CH1 for camera trigger (303re tim1_ch1, nucleo64 CN9-8, uno D7)
// * Potential future enhancement: USB PA11, PA12 (CN10-14, CN10-12)

// TODO:
// NOTE added later than below: See this https://github.com/japaric/stm32f103xx-hal/issues/116
// We have jitter on the order of 5 usec on the primary camera trigger which we could eliminate
// if the camera trigger used PWM on timer1. However, I tried for a full day to get the PWM working
// on timer1 and always failed. Therefore, this code uses the interrupt handler on timer1 to
// turn on the trigger pulse. (Timer7 is used to turn off the trigger pulse.) It should be
// noted, however, that I added support for TIM1 to the stm32nucleo_hal crate and perhaps I did something
// wrong there.

// TODO: check memory layout.
//   See https://stackoverflow.com/questions/44443619/how-to-write-read-to-flash-on-stm32f4-cortex-m4

#![no_main]
#![no_std]

use defmt_rtt as _; // global logger
use panic_probe as _;

use stm32f3xx_hal;
use stm32f3xx_hal as stm32_hal;

use embedded_hal::PwmPin;

use embedded_hal::digital::v2::OutputPin;

use stm32_hal::flash::FlashExt;
use stm32_hal::gpio::GpioExt;
use stm32_hal::gpio::{self, AF7};
use stm32f3xx_hal::prelude::*;

use embedded_time::rate::Hertz;
use stm32_hal::gpio::{Output, PushPull};
use stm32_hal::pac::USART2;
use stm32_hal::pwm::tim3;
use stm32_hal::serial::{Event, Rx, Serial, Tx};

use defmt::{error, info, trace};

use rtic::Mutex;

use mini_rxtx::Decoded;

use crate::stm32_hal::gpio::gpioa::PA5;
use led_box_comms::{ChannelState, DeviceState, FromDevice, OnState, ToDevice};

pub type UserLED = PA5<Output<PushPull>>;

const ZERO_INTENSITY: u16 = 0;
const LED_PWM_FREQ: Hertz = Hertz(500);

#[rtic::app(device = stm32f3xx_hal::pac, peripherals = true)]
mod app {
    use super::*;

    // Late resources
    #[shared]
    struct Shared {
        inner_led_state: InnerLedState,
        rxtx: mini_rxtx::MiniTxRx<
            Rx<USART2, gpio::PA3<AF7<PushPull>>>,
            Tx<USART2, gpio::PA2<AF7<PushPull>>>,
            128,
            128,
        >,
        green_led: UserLED,
        pwm3_ch1: stm32_hal::pwm::PwmChannel<stm32_hal::pwm::Tim3Ch1, stm32_hal::pwm::WithPins>,
        pwm3_ch2: stm32_hal::pwm::PwmChannel<stm32_hal::pwm::Tim3Ch2, stm32_hal::pwm::WithPins>,
        pwm3_ch3: stm32_hal::pwm::PwmChannel<stm32_hal::pwm::Tim3Ch3, stm32_hal::pwm::WithPins>,
        pwm3_ch4: stm32_hal::pwm::PwmChannel<stm32_hal::pwm::Tim3Ch4, stm32_hal::pwm::WithPins>,
    }

    #[local]
    struct Local {}

    #[init]
    fn init(c: init::Context) -> (Shared, Local, init::Monotonics) {
        // Device specific peripherals
        info!("hello from f303");

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
            9_600_u32.Bd(),
            clocks,
            &mut rcc.apb1,
        );
        serial.enable_interrupt(Event::ReceiveDataRegisterNotEmpty);
        // serial.enable_interrupt(Event::TransmitDataRegisterEmtpy); // TODO I am confused why this is not needed.
        let (tx, rx) = serial.split();

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
            use stm32_hal::gpio::gpiob::PB4;
            use stm32_hal::gpio::{Output, PushPull};

            let mut extra_ir_led: PB4<Output<PushPull>> = gpiob
                .pb4
                .into_push_pull_output(&mut gpiob.moder, &mut gpiob.otyper);

            extra_ir_led.set_high().unwrap();
        }

        // initialization of late resources
        (
            Shared {
                inner_led_state: InnerLedState::default(),
                rxtx: mini_rxtx::MiniTxRx::new(tx, rx),
                green_led,
                pwm3_ch1,
                pwm3_ch2,
                pwm3_ch3,
                pwm3_ch4,
            },
            Local {},
            init::Monotonics(),
        )
    }

    #[idle(shared = [rxtx, inner_led_state, pwm3_ch1, pwm3_ch2, pwm3_ch3, pwm3_ch4, green_led])]
    fn idle(mut c: idle::Context) -> ! {
        let mut decode_buf = [0u8; 256];
        let mut decoder = mini_rxtx::Decoder::new(&mut decode_buf);
        let mut encode_buf: [u8; 32] = [0; 32];

        let mut current_device_state = DeviceState::default();

        info!("starting idle loop");
        loop {
            let maybe_byte = c.shared.rxtx.lock(|x| x.pump());
            if let Some(byte) = maybe_byte {
                trace!("got byte: {}", byte);
                // process byte
                match decoder.consume::<led_box_comms::ToDevice>(byte) {
                    Decoded::Msg(ToDevice::DeviceState(next_state)) => {
                        // info!("new state received");

                        update_device_state(&mut current_device_state, &next_state, &mut c);
                        // info!("set new state");
                    }
                    Decoded::Msg(ToDevice::EchoRequest8(buf)) => {
                        let response = FromDevice::EchoResponse8(buf);
                        let msg = mini_rxtx::serialize_msg(&response, &mut encode_buf).unwrap();
                        c.shared.rxtx.lock(|sender| {
                            sender.send_msg(msg).unwrap();
                        });

                        // rtic::pend(pac::Interrupt::USART2_EXTI26);
                        info!("echo");
                    }
                    Decoded::FrameNotYetComplete => {
                        // Frame not complete yet, do nothing until next byte.
                    }
                    Decoded::Error(_) => {
                        panic!("error reading frame");
                    }
                }
            } else {
                // TODO: fix things so we can do this. Right we busy-loop.
                // // no byte to process: go to sleep and wait for interrupt
                // cortex_m::asm::wfi();
            }
        }
    }

    #[task(binds = USART2_EXTI26, shared = [rxtx, green_led])]
    fn usart2_exti26(mut c: usart2_exti26::Context) {
        use stm32f3xx_hal::serial::Error::*;
        c.shared.rxtx.lock(|x| match x.on_interrupt() {
            Ok(()) => {}
            Err(Framing) => {
                error!("serial Framing error.");
                // x.rx().clear_framing_error();
            }
            Err(Noise) => {
                error!("serial Noise error");
            }
            Err(Overrun) => {
                error!("serial Overrun error");
            }
            Err(Parity) => {
                error!("serial Parity error");
            }
            Err(_) => {
                error!("serial unknown error");
            }
        });
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
            info!(
                "setting channel {} to period {}",
                next_state.num, pwm_period
            );
            match next_state.num {
                1 => ctx.shared.pwm3_ch1.lock(|chan| chan.set_duty(pwm_period)),
                2 => ctx.shared.pwm3_ch2.lock(|chan| chan.set_duty(pwm_period)),
                3 => ctx.shared.pwm3_ch3.lock(|chan| chan.set_duty(pwm_period)),
                4 => ctx.shared.pwm3_ch4.lock(|chan| chan.set_duty(pwm_period)),
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
