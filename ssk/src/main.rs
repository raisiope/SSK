// #![deny(warnings)]
#![no_std]
#![no_main]

#[allow(unused)]
use embedded_hal::digital::v2::OutputPin;
use embedded_hal::adc::OneShot;
use core::fmt::Write as FmtWrite;
use embedded_hal::blocking::i2c::{Read, Write};
use fugit::RateExtU32;
use panic_halt as _;
use rp_pico::hal::prelude::*;
use rp_pico::hal::pac;
use rp_pico::hal;
use rp_pico::hal::uart::{DataBits, StopBits, UartConfig};
use bmp280;

#[rp_pico::entry]
fn main() -> ! {
    let mut pac = pac::Peripherals::take().unwrap();
    let core = pac::CorePeripherals::take().unwrap();
    let mut watchdog = hal::Watchdog::new(pac.WATCHDOG);
    let clocks = hal::clocks::init_clocks_and_plls(
        rp_pico::XOSC_CRYSTAL_FREQ,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .ok()
    .unwrap();

    let mut delay = cortex_m::delay::Delay::new(core.SYST, clocks.system_clock.freq().to_Hz());
    let sio = hal::Sio::new(pac.SIO);
    let pins = rp_pico::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    let sda_0_pin = pins.gpio8.into_mode::<hal::gpio::FunctionI2C>();
    let scl_0_pin = pins.gpio9.into_mode::<hal::gpio::FunctionI2C>();

    let mut i2c_0 = hal::I2C::i2c0(
        pac.I2C0,
        sda_0_pin,
        scl_0_pin,
        400.kHz(),
        &mut pac.RESETS,
        &clocks.system_clock,
    );

    let sda_1_pin = pins.gpio10.into_mode::<hal::gpio::FunctionI2C>();
    let scl_1_pin = pins.gpio11.into_mode::<hal::gpio::FunctionI2C>();

    let i2c_1 = hal::I2C::i2c1(
        pac.I2C1,
        sda_1_pin,
        scl_1_pin,
        400.kHz(),
        &mut pac.RESETS,
        &clocks.system_clock,
    );

    let cmd: [u8; 3] = [0xAA, 0x00, 0x00];
    let mut data = [0u8; 4];

    let uart_pins = (
        pins.gpio16.into_mode::<hal::gpio::FunctionUart>(),
        pins.gpio17.into_mode::<hal::gpio::FunctionUart>(),
    );

    let mut uart = hal::uart::UartPeripheral::new(pac.UART0, uart_pins, &mut pac.RESETS)
        .enable(
            UartConfig::new(9600.Hz(), DataBits::Eight, None, StopBits::One),
            clocks.peripheral_clock.freq(),
        )
        .unwrap();

    let mut _led_pin = pins.led.into_push_pull_output();
    let mut ps = bmp280::BMP280::new(i2c_1).unwrap();
    let mut adc = hal::Adc::new(pac.ADC, &mut pac.RESETS);
    let mut adc_pin_0 = pins.gpio26.into_floating_input();
    let mut adc_pin_1 = pins.gpio27.into_floating_input();
    let mut rel0_pin = pins.gpio18.into_push_pull_output();
    let mut rel1_pin = pins.gpio19.into_push_pull_output();

    enum Suunta {
        Ylos,
        Alas,
        Vapaa,
    }

    let mut tavoite_korkeus: u16 = 0;
    let mut suunta = Suunta::Vapaa;

    loop {
        delay.delay_ms(5);
        i2c_0.write(0x18, &cmd).unwrap();
        delay.delay_ms(5);
        i2c_0.read(0x18, &mut data).unwrap();

        let d1 = data[1] as u32;
        let d2 = data[2] as u32;
        let d3 = data[3] as u32;

        let mut raw_psi = (d1 << 16) | (d2 << 8) | d3;
        let psi_min = 0;
        let psi_max = 25;
        raw_psi = (raw_psi - 0x19999A) * (psi_max - psi_min);
        let mut psi = raw_psi as f64;
        psi /= (0xE66666 - 0x19999A) as f64;
        psi += psi_min as f64;
        let hpa = psi * 68.947572932;

        let _temp = ps.temp();
        let pres = ps.pressure();
        let vesi = (hpa - pres / 100f64) * 0.0102;
        let pin_adc1: u16 = adc.read(&mut adc_pin_0).unwrap();
        let pin_adc2: u16 = adc.read(&mut adc_pin_1).unwrap();
        let debug = tavoite_korkeus;
        writeln!(
            uart,
            "vesi:{:.2}:M1:{:02}:M2:{:02}:DEBUG:{:02}",
            vesi, pin_adc1, pin_adc2, debug
        )
        .unwrap();

        let mut buff = [0u8; 6];

        delay.delay_ms(1_000);
        _led_pin.set_high().unwrap();
        delay.delay_ms(100);
        _led_pin.set_low().unwrap();

        while let Ok(_byte) = uart.read_raw(&mut buff) {
            rel1_pin.set_low().unwrap();
            suunta = Suunta::Vapaa;
            if buff[0] == b'R' {
                if buff[1] == b'0' {
                    if buff[2] == b'1' {
                        rel0_pin.set_high().unwrap();
                    } else {
                        rel0_pin.set_low().unwrap();
                    }
                } else if buff[1] == b'1' {
                    if buff[2] == b'1' {
                        rel1_pin.set_high().unwrap();
                    } else {
                        rel1_pin.set_low().unwrap();
                    }
                } else {
                }
            } else {
                let mut arvo: u16 = 0;
                for x in buff {
                    if x.is_ascii_digit() {
                        arvo = 10 * arvo + ((x - b'0') as u16);
                    }
                }
                if (arvo > 0u16) & (arvo < 4000u16) {
                    tavoite_korkeus = arvo;
                    if tavoite_korkeus > pin_adc2 {
                        rel0_pin.set_high().unwrap();
                        suunta = Suunta::Ylos;
                    } else {
                        rel0_pin.set_low().unwrap();
                        suunta = Suunta::Alas;
                    }
                    rel1_pin.set_high().unwrap();
                }
            };
        }

        match suunta {
            Suunta::Ylos => {
                if tavoite_korkeus < pin_adc2 {
                    rel1_pin.set_low().unwrap();
                    suunta = Suunta::Vapaa
                }
            }
            Suunta::Alas => {
                if tavoite_korkeus > pin_adc2 {
                    rel1_pin.set_low().unwrap();
                    suunta = Suunta::Vapaa
                }
            }
            _ => {}
        }

        delay.delay_ms(1_000);
        _led_pin.set_high().unwrap();
        delay.delay_ms(100);
        _led_pin.set_low().unwrap();

        ps.set_control(bmp280::Control {
            osrs_t: bmp280::Oversampling::x1,
            osrs_p: bmp280::Oversampling::x1,
            mode: bmp280::PowerMode::Forced,
        });
    }
}
