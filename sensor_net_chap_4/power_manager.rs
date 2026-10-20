use core::cell::RefCell;
use anyhow::{Result, anyhow};
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex};
use esp_hal::i2c::master::I2c;
use ina3221_dd::{INA3221_I2C_ADDR_GND, Ina3221Async};
use ina3221_dd::ChannelId;

type I2cBus = Mutex<NoopRawMutex, I2c<'static, esp_hal::Async>>;

const BAT_VOLTAGE_MIN: f32 = 3.0;
const BAT_VOLTAGE_MAX: f32 = 4.2; 

pub struct RawData {
  ch1_v: f32,
  ch1_c: f32,
  ch2_v: f32,
  ch2_c: f32,
  ch3_v: f32,
  ch3_c: f32
}

/// Device power management module
pub struct PowerManager {
  /// INA3221 driver
  ina: RefCell<Ina3221Async<ina3221_dd::Ina3221Interface<I2cDevice<'static, NoopRawMutex, I2c<'static, esp_hal::Async>>>,embassy_embedded_hal::shared_bus::I2cDeviceError<esp_hal::i2c::master::Error>>>
}

impl PowerManager {
  /// Creates new instance of the power manager
  /// 
  /// #Arguments
  /// `i2c_bus` - The I2C bus on which the INA3221 is located
  /// 
  /// #Return instance of the manager
  pub fn new(i2c_bus: &'static I2cBus) -> Self {
    let ina_i2c = I2cDevice::new(i2c_bus);
    let ina = Ina3221Async::new(ina_i2c, INA3221_I2C_ADDR_GND);
    
    PowerManager {
      ina: RefCell::new(ina)
    }
  }

  /// Configures the device's power supply hardware. This function must be called
  /// before beginning any work with the manager.
  pub async fn confiure(&self) {
    let mut ina = self.ina.borrow_mut();
      let _ = ina.ll.configuration().modify_async(|w| {
        w.set_ch_1_enable(true);
        w.set_ch_2_enable(true);
        w.set_ch_3_enable(true);
        w.set_averaging_mode(2); 
        w.set_vbus_conversion_time(4); 
        w.set_vshunt_conversion_time(4); 
        w.set_operating_mode(7); 
      }).await;
  }
  
  /// Returns V*A characteristic of supply between solar panel and MPTT contoller
  /// 
  /// #Return VA of error if INA3221 was failed
  pub async fn get_solar_va(&self) -> Result<f32> {
    let mut ina = self.ina.borrow_mut();
    let v3 = ina.get_bus_voltage_mv(ChannelId::Channel3)
      .await
      .map_err(|e| anyhow!("PM error, voltage channel 3 failed: {}", e))?;
    let c3 = ina.get_current_ma(ChannelId::Channel3, 100.0)
      .await
      .map_err(|e| anyhow!("PM error, current channel 3 failed: {}", e))?;

    Ok(v3 * c3 / 1000000.0)
  }
  
  /// Returns V*A characteristic of supply charging contoller and battary
  /// 
  /// #Return VA of error if INA3221 was failed
  pub async fn get_charging_va(&self) -> Result<f32> {
    let mut ina = self.ina.borrow_mut();
    let v2 = ina.get_bus_voltage_mv(ChannelId::Channel2)
      .await
      .map_err(|e| anyhow!("PM error, voltage channel 2 failed: {}", e))?;
    let c2 = ina.get_current_ma(ChannelId::Channel2, 100.0)
      .await
      .map_err(|e| anyhow!("PM error, current channel 2 failed: {}", e))?;

    Ok(v2 * c2 / 1000000.0)
  }
  
  /// Returns battary change in percentage
  /// 
  /// #Return percentage of error if INA3221 was failed
  pub async fn get_battary_charge(&self) -> Result<f32> {
    let mut ina = self.ina.borrow_mut();
    let mut voltage = ina.get_bus_voltage_mv(ChannelId::Channel1)
      .await
      .map_err(|e| anyhow!("PM error, voltage channel 1 failed: {}", e))?;
    voltage = voltage / 1000.0;

    if voltage >= BAT_VOLTAGE_MAX {
        return Ok(100.0);
    }
    if voltage <= BAT_VOLTAGE_MIN {
        return Ok(0.0);
    }
    let charge = ((voltage - BAT_VOLTAGE_MIN) / (BAT_VOLTAGE_MAX - BAT_VOLTAGE_MIN)) * 100.0;

    Ok(charge)
  }

    
  /// Returns V*A characteristic of the all device consumtion
  /// 
  /// #Return VA of error if INA3221 was failed
  pub async fn get_device_consumtion(&self) -> Result<f32> {
    let mut ina = self.ina.borrow_mut();
    let v1 = ina.get_bus_voltage_mv(ChannelId::Channel1)
      .await
      .map_err(|e| anyhow!("PM error, voltage channel 1 failed: {}", e))?;
    let c1 = ina.get_current_ma(ChannelId::Channel1, 100.0)
      .await
      .map_err(|e| anyhow!("PM error, current channel 1 failed: {}", e))?;

    Ok(v1 * c1 / 1000000.0)
  }

  /// Returns raw measured data from power managment sensors
  /// 
  /// #Return struct with raw data of error if INA3221 was failed
  pub async fn get_raw_data(&self) -> Result<RawData> {
    let mut ina = self.ina.borrow_mut();
    let ch1_v = ina.get_bus_voltage_mv(ChannelId::Channel1)
      .await
      .map_err(|e| anyhow!("PM error, voltage channel 1 failed: {}", e))?;
    let ch1_c = ina.get_current_ma(ChannelId::Channel1, 100.0)
      .await
      .map_err(|e| anyhow!("PM error, current channel 1 failed: {}", e))?;
    let ch2_v = ina.get_bus_voltage_mv(ChannelId::Channel2)
      .await
      .map_err(|e| anyhow!("PM error, voltage channel 2 failed: {}", e))?;
    let ch2_c = ina.get_current_ma(ChannelId::Channel2, 100.0)
      .await
      .map_err(|e| anyhow!("PM error, current channel 2 failed: {}", e))?;
    let ch3_v = ina.get_bus_voltage_mv(ChannelId::Channel3)
      .await
      .map_err(|e| anyhow!("PM error, voltage channel 3 failed: {}", e))?;
    let ch3_c = ina.get_current_ma(ChannelId::Channel3, 100.0)
      .await
      .map_err(|e| anyhow!("PM error, current channel 3 failed: {}", e))?;
    
    Ok(RawData { ch1_c, ch1_v, ch2_c, ch2_v, ch3_c, ch3_v })
  }
}