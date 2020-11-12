
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

#[cfg(feature="itm")]
extern crate panic_itm;

#[cfg(feature="semihosting")]
extern crate panic_semihosting;

#[cfg(feature="itm")]
use cortex_m::iprintln;

#[cfg(not(feature="itm"))]
macro_rules! iprintln {
    ($channel:expr) => {
    };
    ($channel:expr, $fmt:expr) => {
    };
    ($channel:expr, $fmt:expr, $($arg:tt)*) => {
    };
}

use stm32f3xx_hal as stm32_hal;

use embedded_hal::PwmPin;

use camtrig_firmware::WrappedTx;

use cast::{u16, u32};
use stm32_hal::stm32::Interrupt;

use embedded_hal::digital::v2::OutputPin;

use stm32_hal::gpio::gpioa::PA8;
use stm32_hal::gpio::GpioExt;
use stm32_hal::prelude::_stm32f3xx_hal_rcc_RccExt;
use stm32_hal::flash::FlashExt;

use stm32_hal::gpio::{Output, PushPull};
use stm32_hal::serial::{self, Rx, Serial};
use stm32_hal::stm32::{self, USART2};
use stm32_hal::time::{Hertz, U32Ext};
use stm32_hal::timer::{self, Timer};
use stm32_hal::pwm::tim3;

use rtfm::Mutex;

use mini_rxtx::Decoded;

use camtrig_comms::{ToDevice, FromDevice, DeviceState, TriggerState, ChannelState, Running,
                    OnState};

const ZERO_INTENSITY: u16 = 0;
const DEFAULT_CAM_TRIG_FREQ: Hertz = Hertz(100);
const SERVICE_LED_PULSE_TRAIN_FREQ: Hertz = Hertz(100);
const LED_PWM_FREQ: Hertz = Hertz(500);
const CAMTRIG_FREQ: Hertz = Hertz(10000); // 0.1 msec cam trig pulses

pub type CamtrigPinType = PA8<Output<PushPull>>;

/// compound type to lock tim1 and trigger count access together
pub struct Timer1TriggerCount {
    tim1: Timer<stm32::TIM1>,
    trigger_count: u64,
}

#[cfg(feature="itm")]
type ItmType = cortex_m::peripheral::ITM;

#[cfg(not(feature="itm"))]
type ItmType = ();

#[rtfm::app(device = stm32f3xx_hal::stm32, peripherals = true)]
const APP: () = {
    // Late resources
    struct Resources {
        inner_led_state: InnerLedState,
        led_pulse_clock_count: u16,
        rxtx: mini_rxtx::MiniTxRx<Rx<USART2>,WrappedTx>,
        trigger_count_timer1: Timer1TriggerCount,
        timer2: Timer<stm32::TIM2>,
        timer7: Timer<stm32::TIM7>,
        camtrig_pin: CamtrigPinType,
        green_led: camtrig_firmware::led::UserLED,
        pwm3_ch1: stm32_hal::pwm::PwmChannel<stm32_hal::pwm::TIM3_CH1,stm32_hal::pwm::WithPins>,
        pwm3_ch2: stm32_hal::pwm::PwmChannel<stm32_hal::pwm::TIM3_CH2,stm32_hal::pwm::WithPins>,
        pwm3_ch3: stm32_hal::pwm::PwmChannel<stm32_hal::pwm::TIM3_CH3,stm32_hal::pwm::WithPins>,
        pwm3_ch4: stm32_hal::pwm::PwmChannel<stm32_hal::pwm::TIM3_CH4,stm32_hal::pwm::WithPins>,
        itm: ItmType,
    }

    #[init]
    fn init(c: init::Context) -> init::LateResources {

        // Device specific peripherals
        let device: stm32_hal::stm32::Peripherals = c.device;

        #[cfg(feature="itm")]
        let mut itm = c.core.ITM;

        #[cfg(not(feature="itm"))]
        let itm = ();

        iprintln!(&mut itm.stim[0], "hello from f303");

        let mut flash = device.FLASH.constrain();
        let mut rcc = device.RCC.constrain();
        let mut gpioa = device.GPIOA.split(&mut rcc.ahb);
        let mut gpiob = device.GPIOB.split(&mut rcc.ahb);
        let clocks = rcc.cfgr.freeze(&mut flash.acr);

        // initialize serial
        let tx = gpioa.pa2.into_af7(&mut gpioa.moder, &mut gpioa.afrl);
        #[cfg(feature = "nucleo64")]
        let rx = gpioa.pa3.into_af7(&mut gpioa.moder, &mut gpioa.afrl);
        #[cfg(feature = "nucleo32")]
        let rx = gpioa.pa15.into_af7(&mut gpioa.moder, &mut gpioa.afrl);

        let mut serial = Serial::usart2(
            device.USART2,
            (tx, rx),
            9_600.bps(),
            clocks,
            &mut rcc.apb1,
        );
        serial.listen(serial::Event::Rxne);
        // serial.listen(serial::Event::Txe); // TODO I am confused why this is not needed.
        let (tx, rx) = serial.split();

        // initialize timer1
        let mut timer1 = Timer::tim1(device.TIM1, DEFAULT_CAM_TRIG_FREQ, clocks, &mut rcc.apb2);
        timer1.listen(timer::Event::Update);

        // initialize timer2
        let mut timer2 = Timer::tim2(device.TIM2, SERVICE_LED_PULSE_TRAIN_FREQ, clocks, &mut rcc.apb1);
        timer2.listen(timer::Event::Update);

        iprintln!(&mut itm.stim[0], "initializing tim3 for pwm");

        // initialize pwm3
        let clock_freq = clocks.pclk1().0 * if clocks.ppre1() == 1 { 1 } else { 2 };

        let ticks = clock_freq / LED_PWM_FREQ.0;

        let psc = u16((ticks - 1) / (1 << 16)).unwrap();
        let arr = u16(ticks / u32(psc + 1)).unwrap(); // camtrig_comms::MAX_INTENSITY

        let (ch1_no_pins, ch2_no_pins, ch3_no_pins, ch4_no_pins) =
            tim3(device.TIM3, arr, psc);

        iprintln!(&mut itm.stim[0], "arr {}, camtrig_comms::MAX_INTENSITY {}", arr, camtrig_comms::MAX_INTENSITY);
        iprintln!(&mut itm.stim[0], "initializing pwm pins");

        let pa6 = gpioa.pa6.into_af2(&mut gpioa.moder, &mut gpioa.afrl); // ch1
        let pa7 = gpioa.pa7.into_af2(&mut gpioa.moder, &mut gpioa.afrl); // ch2
        let pb0 = gpiob.pb0.into_af2(&mut gpiob.moder, &mut gpiob.afrl); // ch3
        let pb1 = gpiob.pb1.into_af2(&mut gpiob.moder, &mut gpiob.afrl); // ch4

        let mut pwm3_ch1 = ch1_no_pins
            .output_to_pa6(pa6);
        let mut pwm3_ch2 = ch2_no_pins
            .output_to_pa7(pa7);
        let mut pwm3_ch3 = ch3_no_pins
            .output_to_pb0(pb0);
        let mut pwm3_ch4 = ch4_no_pins
            .output_to_pb1(pb1);

        pwm3_ch1.enable();
        pwm3_ch2.enable();
        pwm3_ch3.enable();
        pwm3_ch4.enable();
        // pwm3.listen() not called, so no interrupt will fire.

        iprintln!(&mut itm.stim[0], "initializing timer7");

        // initialize timer7
        let mut timer7 = Timer::tim7(device.TIM7, CAMTRIG_FREQ, clocks, &mut rcc.apb1);
        timer7.listen(timer::Event::Update);

        // initialize camera trigger pin
        let camtrig_pin = gpioa.pa8
            .into_push_pull_output(&mut gpioa.moder, &mut gpioa.otyper);

        #[cfg(feature = "nucleo64")]
        let mut green_led = gpioa.pa5
            .into_push_pull_output(&mut gpioa.moder, &mut gpioa.otyper);
        #[cfg(feature = "nucleo32")]
        let mut green_led = gpiob.pb3
            .into_push_pull_output(&mut gpiob.moder, &mut gpiob.otyper);
        green_led.set_high().unwrap();

        {
            use stm32_hal::gpio::gpiob::PB4;
            use stm32_hal::gpio::{Output, PushPull};

            let mut extra_ir_led: PB4<Output<PushPull>> = gpiob.pb4
                .into_push_pull_output(&mut gpiob.moder, &mut gpiob.otyper);

            extra_ir_led.set_high().unwrap();
        }

        // initialization of late resources
        init::LateResources {
            inner_led_state: InnerLedState::default(),
            led_pulse_clock_count: 0,
            rxtx: mini_rxtx::MiniTxRx::new(WrappedTx{tx},rx),
            trigger_count_timer1: Timer1TriggerCount {
                tim1: timer1,
                trigger_count: 0,
            },
            timer2,
            timer7,
            camtrig_pin,
            green_led,
            pwm3_ch1,
            pwm3_ch2,
            pwm3_ch3,
            pwm3_ch4,
            itm,
        }
    }

    #[idle(resources = [rxtx, trigger_count_timer1, inner_led_state, pwm3_ch1, pwm3_ch2, pwm3_ch3, pwm3_ch4])]
    fn idle(mut c: idle::Context) -> ! {
        let mut decode_buf = [0u8; 256];
        let mut decoder = mini_rxtx::Decoder::new(&mut decode_buf);
        let mut encode_buf: [u8; 32] = [0; 32];

        let mut current_device_state = DeviceState::default();

        loop {

            let maybe_byte = c.resources.rxtx.lock(|x| x.pump());
            if let Some(byte) = maybe_byte {
                // process byte
                match decoder.consume::<camtrig_comms::ToDevice>(byte) {

                    Decoded::Msg(ToDevice::DeviceState(next_state)) => {
                        // iprintln!(&itm.stim[0], "new state received");

                        update_device_state(&mut current_device_state, &next_state,
                            &mut c.resources);
                        // iprintln!(&itm.stim[0], "set new state");
                    }
                    Decoded::Msg(ToDevice::EchoRequest8(buf)) => {
                        let response = FromDevice::EchoResponse8(buf);
                        let msg = mini_rxtx::serialize_msg(&response, &mut encode_buf).unwrap();
                        c.resources.rxtx.lock( |sender| {
                            sender.send_msg(msg).unwrap();
                        });

                        rtfm::pend(Interrupt::USART2_EXTI26);
                        // iprintln!(&itm.stim[0], "echo");
                    }
                    Decoded::Msg(ToDevice::CounterInfoRequest(_tim_num)) => {
                        unimplemented!();
                    }
                    Decoded::Msg(ToDevice::TimerRequest) => {
                        let (trigger_count, tim1_cnt) = {
                            c.resources.trigger_count_timer1.lock(|ddx| {
                                let tim1_cnt = ddx.tim1.counter();
                                (ddx.trigger_count.clone(), u16(tim1_cnt).unwrap())
                            })
                        };
                        let response = FromDevice::TimerResponse((trigger_count, tim1_cnt));
                        let msg = mini_rxtx::serialize_msg(&response, &mut encode_buf).unwrap();
                        c.resources.rxtx.lock( |sender| {
                            sender.send_msg(msg).unwrap();
                        });

                        rtfm::pend(Interrupt::USART2_EXTI26);
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

    #[task(binds = USART2_EXTI26, resources = [rxtx])]
    fn usart2_exti26(c: usart2_exti26::Context) {
        c.resources.rxtx.on_interrupt();
    }

    #[task(binds = TIM1_UP_TIM16, resources = [trigger_count_timer1, camtrig_pin, timer7])]
    fn my_tim1_up_tim16(mut c: my_tim1_up_tim16::Context) {
        c.resources.trigger_count_timer1.tim1.clear_update_interrupt_flag();

        c.resources.trigger_count_timer1.trigger_count += 1;
        c.resources.camtrig_pin.set_high().unwrap();
        c.resources.timer7.resume();
    }

    #[task(binds = TIM2, resources = [timer2, led_pulse_clock_count, inner_led_state, pwm3_ch1, pwm3_ch2, pwm3_ch3, pwm3_ch4])]
    fn tim2(c: tim2::Context) {
        c.resources.timer2.clear_update_interrupt_flag();

        let clock_val = {
            *c.resources.led_pulse_clock_count += 1;
            *c.resources.led_pulse_clock_count
        };

        service_channel_pulse_train( clock_val, c.resources.pwm3_ch1, &mut c.resources.inner_led_state.ch1);
        service_channel_pulse_train( clock_val, c.resources.pwm3_ch2, &mut c.resources.inner_led_state.ch2);
        service_channel_pulse_train( clock_val, c.resources.pwm3_ch3, &mut c.resources.inner_led_state.ch3);
        service_channel_pulse_train( clock_val, c.resources.pwm3_ch4, &mut c.resources.inner_led_state.ch4);
    }

    #[task(binds = TIM7, resources = [timer7, camtrig_pin])]
    fn tim7(c: tim7::Context) {
        c.resources.timer7.clear_update_interrupt_flag();
        c.resources.timer7.stop();
        c.resources.timer7.reset_counter();

        c.resources.camtrig_pin.set_low().unwrap();
    }

};

#[derive(Debug, PartialEq, Clone, Copy)]
enum MyChan {
    Ch1,
    Ch2,
    Ch3,
    Ch4,
}

fn service_channel_pulse_train<X>(clock_val: u16, channel: &mut X, inner_chan_state: &mut InnerLedChannelState)
    where
        X: PwmPin<Duty=u16>,
{
    let mut new_pwm3_info = None;

    {
        let next_mode = match inner_chan_state.mode {
            Mode::Immediate(v) => {
                Mode::Immediate(v)
            },
            Mode::StartingPulseTrain((params,pwm_period)) => {
                let stop_clock = clock_val + u16(params.pulse_dur_ticks).unwrap();
                new_pwm3_info = Some((channel, pwm_period));
                Mode::OngoingPulse((stop_clock,pwm_period))
            },
            Mode::OngoingPulse((stop_clock,pwm_period)) => {
                // TODO deal with clock wraparound
                // TODO deal with pulse trains not just single pulse
                if clock_val >= stop_clock {
                    new_pwm3_info = Some((channel, ZERO_INTENSITY));
                    Mode::Immediate(ZERO_INTENSITY)
                } else {
                    Mode::OngoingPulse((stop_clock,pwm_period))
                }
            }
        };
        inner_chan_state.mode = next_mode;

    }

    if let Some((channel,pwm_period)) = new_pwm3_info {
        // actually change LED intensity
        channel.set_duty(pwm_period);
    }
}

fn update_trigger_state(
    next_state: &TriggerState,
    resources: &mut idle::Resources,
) {

    resources.trigger_count_timer1.lock( |trigger_count_timer1| {

    match next_state.running {
        Running::Stopped => {
            trigger_count_timer1.tim1.stop();
        }
        Running::ConstantFreq(freq_hz) => {
            let timeout = stm32_hal::time::Hertz(freq_hz as u32);
            use embedded_hal::timer::CountDown;
            // XXX FIXME TODO check will this repeat, or is it single-shot?
            trigger_count_timer1.tim1.start(timeout);
        }
    }
    })
}

fn update_led_state(
    next_state: &ChannelState,
    resources: &mut idle::Resources,
) {

    let mut set_pwm3_now = None;
    {
        resources.inner_led_state.lock( |inner_led_state| {

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
                OnState::Off => 0,
                OnState::ConstantOn => next_state.intensity,
                OnState::PulseTrain(_) => next_state.intensity,
            };

            // Based on on_state, decide what to do.
            match next_state.on_state {
                OnState::Off | OnState::ConstantOn => {
                    set_pwm3_now = Some(pwm_period);
                    inner_led_chan_state.mode = Mode::Immediate(pwm_period);
                }
                OnState::PulseTrain(pt) => {
                    inner_led_chan_state.mode = Mode::StartingPulseTrain((pt,pwm_period));
                },
            }
        })
    }
    if let Some(pwm_period) = set_pwm3_now {
        match next_state.num {
            1 => resources.pwm3_ch1.lock(|chan| chan.set_duty(pwm_period)),
            2 => resources.pwm3_ch2.lock(|chan| chan.set_duty(pwm_period)),
            3 => resources.pwm3_ch3.lock(|chan| chan.set_duty(pwm_period)),
            4 => resources.pwm3_ch4.lock(|chan| chan.set_duty(pwm_period)),
            _ => panic!("unknown channel"),
        };
    }
    rtfm::pend(Interrupt::TIM2);
}

// -------------------------------------------------------------------------
// -------------------------------------------------------------------------
// -------------------------------------------------------------------------
/// update the outer level state (On, ConstantOn, ) into the inner state machine.


/// This keeps track of our actual low-level state
#[derive(Debug, PartialEq, Clone, Copy)]
enum Mode {
    Immediate(u16), // pwm_period
    StartingPulseTrain((camtrig_comms::PulseTrainParams,u16)),
    OngoingPulse((u16,u16)), // stop clock time
}

/// This keeps track of our actual low-level state
#[derive(Debug, PartialEq, Clone, Copy)]
struct InnerLedChannelState {
    tim3_channel: MyChan,
    mode: Mode,
}

impl InnerLedChannelState {
    const fn default(tim3_channel: MyChan) -> Self {
        Self {
            tim3_channel,
            mode: Mode::Immediate(0),
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

fn update_device_state(current_state: &mut DeviceState,
                       next_state: &DeviceState,
                       mut resources: &mut idle::Resources,
                       ) {
    if current_state.trig != next_state.trig {
        update_trigger_state(&next_state.trig, &mut resources);
        current_state.trig = next_state.trig;
    }
    if current_state.ch1 != next_state.ch1 {
        update_led_state(&next_state.ch1, &mut resources);
        current_state.ch1 = next_state.ch1;
    }
    if current_state.ch1 != next_state.ch1 {
        update_led_state(&next_state.ch1, &mut resources);
        current_state.ch1 = next_state.ch1;
    }
    if current_state.ch2 != next_state.ch2 {
        update_led_state(&next_state.ch2, &mut resources);
        current_state.ch2 = next_state.ch2;
    }
    if current_state.ch3 != next_state.ch3 {
        update_led_state(&next_state.ch3, &mut resources);
        current_state.ch3 = next_state.ch3;
    }
    if current_state.ch4 != next_state.ch4 {
        update_led_state(&next_state.ch4, &mut resources);
        current_state.ch4 = next_state.ch4;
    }
}
