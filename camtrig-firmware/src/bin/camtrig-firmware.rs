// TODO: check if NUCLEO-L152RE would work (STM32L152RET6 microcontroller) it has eeprom
// and NUCLEO-L031K6 (STM32L031K6 microcontroller) also. Update: nevermind, we should
// be able to use the flash to store things in a non-volatile way. There is even EEPROM
// emulation, but this seems overkill.

// Present design for nucleo64 stm32f303re board ("nucleo64" feature):
// * PA6, PA7, PB0, PB1 reserved for optogenetics and backlighting LED control
//  (303re tim3 pwm ch1,ch2,ch3,ch4, nucleo64 CN10-13, CN10-15, CN7-34, CN10-24,
//  uno D12, D11, A3, n.a.)
// * PB4 optional extra IR LED to check tracking latency
// * PA2, PA3 used for serial comms (303re usart2, nucleo64 CN10-35, CN10-37, uno D1, n.a.)
// * PA5 user LED on board (303re gpioa5, nucleo64 CN10-11, uno D13)
// * PA8 used as TIM1_CH1 for camera trigger (303re tim1_ch1, nucleo64 CN9-8, uno D7)
// * Potential future enhancement: USB PA11, PA12 (CN10-14, CN10-12)

// Design for nucleo32 stm32f303k8 board  ("nucleo32" feature):
// * PA6, PA7, PB0, PB1 reserved for optogenetics and backlighting LED control
//  (303k8 tim3 pwm ch1,ch2,ch3,ch4, nucleo32 CN4-7, CN4-6, CN3-6, CN3-9; nano a5, a6, d3, d6)
// * PA2, PA15 used for serial comms (303k8 usart2, nucleo32 CN4-5, none)
// * PB3 user LED on board (303k8 gpiob3, CN4-15)
// * PB4 optional extra IR LED to check tracking latency
// * PA8 used as TIM1_CH1 for camera trigger (303k8 tim1_ch1, nucleo32 CN3-12, nano D9)
// * Future: PA12 extra LED on strawlab triggerbox board (nucleo32 CN3-5, nano D2)
// * Future: PB3 SPI1_SCK to MCP4822 SCK (nucleo32 CN4-15, nano D13)
// * Future: PB5 SPI1_MOSI to MCP4822 SDI (nucleo32 CN3-14, nano D11)
// * Future: PA11 to MCP4822 CS (nucleo32 CN3-13, nano D10)
// * Future: PF0 to MCP4822 LDAC (nucleo32 CN3-10, nano D7)
// * No onboard USB

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

use cast::u16;

use embedded_hal::digital::v2::OutputPin;

use stm32_hal::flash::FlashExt;
use stm32_hal::gpio::GpioExt;
use stm32_hal::gpio::{self, gpioa::PA8, AF7};
use stm32f3xx_hal::prelude::*;

use embedded_time::{
    duration::Microseconds,
    rate::{Hertz, Rate},
};
use stm32_hal::gpio::{Output, PushPull};
use stm32_hal::pac::{self, USART2};
use stm32_hal::pwm::tim3;
use stm32_hal::serial::{Event, Rx, Serial, Tx};
use stm32_hal::timer::{self, Timer};

use defmt::{error, info, trace, warn};

use rtic::Mutex;

use mini_rxtx::Decoded;

use camtrig_comms::{
    ChannelState, DeviceState, FromDevice, OnState, Running, ToDevice, TriggerState,
};

const ZERO_INTENSITY: u16 = 0;
const LED_PWM_FREQ: Hertz = Hertz(500);

#[rtic::app(device = stm32f3xx_hal::pac, peripherals = true)]
mod app {
    use super::*;

    // Late resources
    #[shared]
    struct Shared {
        inner_led_state: InnerLedState,
        // led_pulse_clock_count: u16,
        rxtx: mini_rxtx::MiniTxRx<
            Rx<USART2, gpio::PA3<AF7<PushPull>>>,
            // WrappedTx,
            Tx<USART2, gpio::PA2<AF7<PushPull>>>,
            128,
            128,
        >,
        green_led: camtrig_firmware::led::UserLED,
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
        #[cfg(feature = "nucleo64")]
        let rx = gpioa
            .pa3
            .into_af_push_pull(&mut gpioa.moder, &mut gpioa.otyper, &mut gpioa.afrl);
        #[cfg(feature = "nucleo32")]
        let mut rx =
            gpioa
                .pa15
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
                camtrig_comms::MAX_INTENSITY,
                pwm_freq_hz.0,
            );
        }

        // initialize pwm3
        info!("initializing tim3 for pwm");
        let tim3_channels = tim3(
            c.device.TIM3,
            camtrig_comms::MAX_INTENSITY, // resolution of duty cycle
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

        // info!("initializing timer7");
        // // initialize timer7
        // let mut timer7 = Timer::new(c.device.TIM7, clocks, &mut rcc.apb1);
        // timer7.enable_interrupt(timer::Event::Update);
        // timer7.start::<Microseconds>(CAMTRIG_FREQ.to_duration().unwrap());

        // // initialize camera trigger pin
        // let camtrig_pin = gpioa
        //     .pa8
        //     .into_push_pull_output(&mut gpioa.moder, &mut gpioa.otyper);

        #[cfg(feature = "nucleo64")]
        let mut green_led = gpioa
            .pa5
            .into_push_pull_output(&mut gpioa.moder, &mut gpioa.otyper);
        #[cfg(feature = "nucleo32")]
        let mut green_led = gpiob
            .pb3
            .into_push_pull_output(&mut gpiob.moder, &mut gpiob.otyper);
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
                // led_pulse_clock_count: 0,
                rxtx: mini_rxtx::MiniTxRx::new(tx, rx),
                // trigger_count_timer1: Timer1TriggerCount {
                //     tim1: timer1,
                //     trigger_count: 0,
                // },
                // timer2,
                // timer7,
                // camtrig_pin,
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

    #[idle(shared = [rxtx, inner_led_state, pwm3_ch1, pwm3_ch2, pwm3_ch3, pwm3_ch4])]
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
                match decoder.consume::<camtrig_comms::ToDevice>(byte) {
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
                    Decoded::Msg(ToDevice::CounterInfoRequest(_tim_num)) => {
                        unimplemented!();
                    }
                    Decoded::Msg(ToDevice::TimerRequest) => {
                        let (trigger_count, tim1_cnt) = {
                            todo!();
                            // c.shared.trigger_count_timer1.lock(|ddx| {
                            //     // let tim1_cnt = ddx.tim1.counter();
                            //     let tim1_cnt: u32 = 0;
                            //     error!("unimplemented: tim1.counter()");
                            //     (ddx.trigger_count.clone(), u16(tim1_cnt).unwrap())
                            // })
                        };
                        let response = FromDevice::TimerResponse((trigger_count, tim1_cnt));
                        let msg = mini_rxtx::serialize_msg(&response, &mut encode_buf).unwrap();
                        c.shared.rxtx.lock(|sender| {
                            sender.send_msg(msg).unwrap();
                        });
                        info!("handle timer request");
                        // rtic::pend(pac::Interrupt::USART2_EXTI26);
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
        c.shared
            .green_led
            .lock(|green_led| green_led.set_high().unwrap());
    }

    // #[task(binds = TIM1_UP_TIM16, shared = [trigger_count_timer1, camtrig_pin, timer7])]
    // fn my_tim1_up_tim16(ctx: my_tim1_up_tim16::Context) {
    //     let trigger_count_timer1 = ctx.shared.trigger_count_timer1;
    //     let camtrig_pin = ctx.shared.camtrig_pin;
    //     let timer7 = ctx.shared.timer7;

    //     (trigger_count_timer1, camtrig_pin, timer7).lock(
    //         |trigger_count_timer1, camtrig_pin, timer7| {
    //             trigger_count_timer1.tim1.clear_event(timer::Event::Update);
    //             trigger_count_timer1.trigger_count += 1;
    //             camtrig_pin.set_high().unwrap();
    //             timer7.start::<Microseconds>(CAMTRIG_FREQ.to_duration().unwrap());
    //         },
    //     );
    // }

    // #[task(binds = TIM2, shared = [timer2, led_pulse_clock_count, inner_led_state, pwm3_ch1, pwm3_ch2, pwm3_ch3, pwm3_ch4])]
    // fn tim2(c: tim2::Context) {
    //     let led_pulse_clock_count = c.shared.led_pulse_clock_count;
    //     let timer2 = c.shared.timer2;
    //     let pwm3_ch1 = c.shared.pwm3_ch1;
    //     let pwm3_ch2 = c.shared.pwm3_ch2;
    //     let pwm3_ch3 = c.shared.pwm3_ch3;
    //     let pwm3_ch4 = c.shared.pwm3_ch4;
    //     let inner_led_state = c.shared.inner_led_state;

    //     (
    //         timer2,
    //         led_pulse_clock_count,
    //         pwm3_ch1,
    //         pwm3_ch2,
    //         pwm3_ch3,
    //         pwm3_ch4,
    //         inner_led_state,
    //     )
    //         .lock(
    //             |timer2,
    //              led_pulse_clock_count,
    //              pwm3_ch1,
    //              pwm3_ch2,
    //              pwm3_ch3,
    //              pwm3_ch4,
    //              inner_led_state| {
    //                 timer2.clear_event(timer::Event::Update);
    //                 *led_pulse_clock_count += 1;
    //                 let clock_val = *led_pulse_clock_count;

    //                 // service_channel_pulse_train(clock_val, pwm3_ch1, &mut inner_led_state.ch1);
    //                 // service_channel_pulse_train(clock_val, pwm3_ch2, &mut inner_led_state.ch2);
    //                 // service_channel_pulse_train(clock_val, pwm3_ch3, &mut inner_led_state.ch3);
    //                 // service_channel_pulse_train(clock_val, pwm3_ch4, &mut inner_led_state.ch4);
    //             },
    //         );
    // }

    // #[task(binds = TIM7, shared = [timer7, camtrig_pin])]
    // fn tim7(c: tim7::Context) {
    //     let timer7 = c.shared.timer7;
    //     let camtrig_pin = c.shared.camtrig_pin;

    //     (timer7, camtrig_pin).lock(|timer7, camtrig_pin| {
    //         timer7.clear_event(timer::Event::Update);
    //         timer7.stop();
    //         // timer7.reset_counter();

    //         camtrig_pin.set_low().unwrap();
    //     });
    // }

    // fn update_trigger_state(next_state: &TriggerState, ctx: &mut idle::Context) {
    //     ctx.shared
    //         .trigger_count_timer1
    //         .lock(|trigger_count_timer1| {
    //             match next_state.running {
    //                 Running::Stopped => {
    //                     trigger_count_timer1.tim1.stop();
    //                 }
    //                 Running::ConstantFreq(freq_hz) => {
    //                     // XXX FIXME TODO check will this repeat, or is it single-shot?
    //                     trigger_count_timer1
    //                         .tim1
    //                         .start::<Microseconds>(Hertz(freq_hz as u32).to_duration().unwrap());
    //                 }
    //             }
    //         })
    // }

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
                    OnState::PulseTrain(_) => {
                        todo!()
                    } //next_state.intensity,
                };

                // Based on on_state, decide what to do.
                match next_state.on_state {
                    OnState::Off | OnState::ConstantOn => {
                        set_pwm3_now = Some(pwm_period);
                        // inner_led_chan_state.mode = Mode::Immediate(pwm_period);
                        inner_led_chan_state.period = pwm_period;
                    }
                    OnState::PulseTrain(pt) => {
                        todo!();
                        // inner_led_chan_state.mode = Mode::StartingPulseTrain((pt, pwm_period));
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
        if current_state.trig != next_state.trig {
            warn!("ignoring requested camera trigger state change.");
            trace!("current_state.trig: {}", current_state.trig);
            trace!("next_state.trig: {}", next_state.trig);
            // update_trigger_state(&next_state.trig, &mut ctx);
            // current_state.trig = next_state.trig;
        }
        if current_state.ch1 != next_state.ch1 {
            update_led_state(&next_state.ch1, &mut ctx);
            current_state.ch1 = next_state.ch1;
        }
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

// fn service_channel_pulse_train<X>(
//     clock_val: u16,
//     channel: &mut X,
//     inner_chan_state: &mut InnerLedChannelState,
// ) where
//     X: PwmPin<Duty = u16>,
// {
//     // let mut new_pwm3_info = None;

//     // {
//     //     let next_mode = match inner_chan_state.mode {
//     //         Mode::Immediate(v) => Mode::Immediate(v),
//     //         // Mode::StartingPulseTrain((params, pwm_period)) => {
//     //         //     let stop_clock = clock_val + u16(params.pulse_dur_ticks).unwrap();
//     //         //     new_pwm3_info = Some((channel, pwm_period));
//     //         //     Mode::OngoingPulse((stop_clock, pwm_period))
//     //         // }
//     //         // Mode::OngoingPulse((stop_clock, pwm_period)) => {
//     //         //     // TODO deal with clock wraparound
//     //         //     // TODO deal with pulse trains not just single pulse
//     //         //     if clock_val >= stop_clock {
//     //         //         new_pwm3_info = Some((channel, ZERO_INTENSITY));
//     //         //         Mode::Immediate(ZERO_INTENSITY)
//     //         //     } else {
//     //         //         Mode::OngoingPulse((stop_clock, pwm_period))
//     //         //     }
//     //         // }
//     //     };
//     //     inner_chan_state.mode = next_mode;
//     // }

//     if let Some((channel, pwm_period)) = new_pwm3_info {
//         // actually change LED intensity
//         channel.set_duty(pwm_period);
//     }
// }

// -------------------------------------------------------------------------
// -------------------------------------------------------------------------
// -------------------------------------------------------------------------
/// update the outer level state (On, ConstantOn, ) into the inner state machine.

// /// This keeps track of our actual low-level state
// #[derive(Debug, PartialEq, Clone, Copy)]
// enum Mode {
//     Immediate(u16), // pwm_period
//     // StartingPulseTrain((camtrig_comms::PulseTrainParams, u16)),
//     // OngoingPulse((u16, u16)), // stop clock time
// }

/// This keeps track of our actual low-level state
#[derive(Debug, PartialEq, Clone, Copy)]
struct InnerLedChannelState {
    tim3_channel: MyChan,
    // mode: Mode,
    period: u16,
}

impl InnerLedChannelState {
    const fn default(tim3_channel: MyChan) -> Self {
        Self {
            tim3_channel,
            period: 0,
            // mode: Mode::Immediate(0),
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
