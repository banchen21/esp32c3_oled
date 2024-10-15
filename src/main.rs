use std::time::{self, SystemTime};

use anyhow::Result;
use chrono::{DateTime, Utc};
use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::Point,
    text::{Baseline, Text},
};
use embedded_hal::blocking::delay::DelayMs;
use embedded_svc::mqtt::client::{EventPayload::Error, EventPayload::Received, QoS};
use esp32c3_wifi::wifi;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::{
        delay::FreeRtos,
        i2c::{I2cConfig, I2cDriver},
        peripherals::Peripherals,
        prelude::*,
    },
    mqtt::client::{EspMqttClient, MqttClientConfiguration, MqttProtocolVersion},
    sntp::{EspSntp, SyncStatus},
    wifi::AuthMethod,
};
use log::{error, info, warn};

use embedded_graphics::Drawable;
use ssd1306::{
    mode::DisplayConfig, prelude::DisplayRotation, size::DisplaySize128x64, I2CDisplayInterface,
    Ssd1306,
};
fn main() -> Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();
    let sysloop = EspSystemEventLoop::take()?;

    let auth_method = AuthMethod::WPA2Personal;
    let _wifi = match wifi(
        "Xiaomi_CF31",
        "zjeasy@123",
        auth_method,
        peripherals.modem,
        sysloop,
    ) {
        Ok(_conn) => _conn,
        Err(_err) => {
            error!("WiFi  连接失败{_err}");
            return Ok(());
        }
    };

    esp_idf_svc::hal::task::block_on(async {
        info!("请求SNTP时间同步");
        let ntp = EspSntp::new_default().unwrap();
        while ntp.get_sync_status() != SyncStatus::Completed {}
        info!("时间同步完成");
    });

    let sda = peripherals.pins.gpio5;
    let scl = peripherals.pins.gpio6;
    let config = I2cConfig::new().baudrate(400.kHz().into());
    let i2c = I2cDriver::new(peripherals.i2c0, sda, scl, &config)?;
    let interface = I2CDisplayInterface::new(i2c);
    let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();
    display.init().unwrap();
    let text_style = MonoTextStyleBuilder::new()
        .font(&FONT_6X10)
        .text_color(BinaryColor::On)
        .build();
    loop {
        // // 睡眠一秒
        // FreeRtos::delay_ms(500);
        // 显示时间
        let st_now = SystemTime::now();
        // Convert to UTC Time
        let dt_now_utc: DateTime<Utc> = st_now.clone().into();
        // Format Time String
        let formatted = format!("{}", dt_now_utc.format("nowtime: %Y-%m-%d %H:%M:%S"));
       
        Text::with_baseline(&formatted, Point::new(0, 32), text_style, Baseline::Top)
            .draw(&mut display)
            .unwrap();
        // 刷新屏幕
        display.flush().unwrap();
        display.clear_buffer();
    }
    
}
