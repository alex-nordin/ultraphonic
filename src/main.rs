#![no_std]
#![no_main]

use ag_lcd::*;
use arduino_hal::port::mode::Floating;
use arduino_hal::port::mode::Input;
use arduino_hal::port::mode::Output;
use arduino_hal::port::Pin;
use avr_device::atmega328p::TC1;
use panic_halt as _;

#[arduino_hal::entry]
fn main() -> ! {
    // get access to peripherals (pins, serial port, EEPROM etc)
    let dp = arduino_hal::Peripherals::take().unwrap();
    let pins = arduino_hal::pins!(dp);
    // configure serial port with default values
    let mut serial = arduino_hal::default_serial!(dp, pins, 57600);

    // HC-SR04 ultrasonic sensor has a trigger pin wired to d9, which triggers a sound wave
    // I took inspiration for how to work with timers and the sensor at :
    // https://github.com/Rahix/avr-hal/blob/190f2c3cb8d29e10f71119352b912369dc5a1fb7/examples/arduino-uno/src/bin/uno-hc-sr04.rs#L4

    let mut trigger = pins.d9.into_output().downgrade();
    // and the echo pin used to listen to the echo of the wave at d8
    let echo = pins.d8.downgrade();

    // TC1 is one of the timer types on the arduino board
    let timer = dp.TC1;
    // the timer is basically a buffer which is incremented, and we prescale with 64 bits
    // making the interval the clock ticks at 4 microseconds, which will fill the clock after 200 ms
    timer.tccr1b.write(|w| w.cs1().prescale_64());

    //data pins for lcd screen
    let d4 = pins.d4.into_output().downgrade();
    let d5 = pins.d5.into_output().downgrade();
    let d6 = pins.d6.into_output().downgrade();
    let d7 = pins.d7.into_output().downgrade();

    //register select pin
    let rs = pins.d12.into_output().downgrade();
    //"enable" pin
    let en = pins.d11.into_output().downgrade();

    // init a Delay struct so lcd has access to delay functions/interrupts
    let delay = arduino_hal::Delay::new();

    let mut lcd: LcdDisplay<_, _> = LcdDisplay::new(rs, en, delay)
        // set the display to 4 pin mode usgin "with_half_bus" function
        .with_half_bus(d4, d5, d6, d7)
        .with_display(Display::On)
        .build();

    // enter the loop of measuring distance and printing it on serial and lcd
    loop {
        // get distance to target of one is within range
        let distance_to_target = measure_distance(&timer, &mut trigger, &echo);
        // the lcd crate I chose can only print &str so we use the itoa crate to efficiently convert u16 to str
        let mut buffer = itoa::Buffer::new();
        let print_distance = buffer.format(distance_to_target);
        // // print on lcd screen
        lcd.print(print_distance);
        lcd.print(" mm");
        // write to serial for debuggin
        ufmt::uwriteln!(&mut serial, "{} mm", distance_to_target as u16).unwrap();
        // clear screen so we don't overlap text on lcd
        // delay at the end of each loop before sending another sonic pulse
        arduino_hal::delay_ms(1000);
        lcd.clear();
    }
}

// measure distance by timing how long it takes for a hypersonic pulse to bounce back to our sensor
// needs to be passed an instance of a TC1 timer and trigger + echo pins.
// I chose to make these pins dynamic and not hardcode which pins are used, which incurs a runtime performance loss
fn measure_distance(
    timer: &TC1,
    trigger_pin: &mut Pin<Output>,
    echo_pin: &Pin<Input<Floating>>,
) -> u16 {
    // reset timer at the start of each measurement
    timer.tcnt1.write(|w| w.bits(0));

    // setting the trigger pin high for 10 us sends out the correct sound pulse
    trigger_pin.set_high();
    arduino_hal::delay_us(10);
    trigger_pin.set_low();

    // the echo pins switches to high when/if our pulse bounces back, so we wait until that happens
    while echo_pin.is_low() {
        // if our timer runs out while we wait for the pulse to return no object has been detected. currently just setting distance as 0
        if timer.tcnt1.read().bits() >= 50_000 {
            let early_return_distance = 0;
            return early_return_distance;
        }
    }

    // we reset the timer here so we have a clean slate to calculate how long the echo pin is set to high
    timer.tcnt1.write(|w| w.bits(0));

    // waiting with timer running until echo pin returns to low
    while echo_pin.is_high() {}

    // reading distance as the value of the timer multiplied by 4, as each clock tick is 4 us
    // saturating mul is multiplication where product cannot exceed a certain threshold
    let distance = timer.tcnt1.read().bits().saturating_mul(4);

    // if echo pin was held in high for too long and it exceeds the bounds of a u16 int we count it as a bad reading and return a 0
    match distance {
        u16::MAX => 0,
        // otherwise calculate the distance from the timer value. TODO: why divide by 58? possibly correct but double check math
        _ => distance / 58,
    };

    distance
}
