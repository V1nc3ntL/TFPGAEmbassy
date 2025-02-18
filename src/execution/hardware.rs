use core::panic;

use embassy_net::DhcpConfig;
use embassy_net::{new, Runner, Stack, StackResources};
use esp_alloc as _;
use esp_backtrace as _;
use esp_hal::Blocking;
use esp_hal::{clock::CpuClock, i2c::master::*, rng::Rng, timer::timg::TimerGroup};
use esp_wifi::wifi::{WifiController, WifiDevice, WifiStaDevice};

use xpowers_axp2101::Pmu;

const NUMBER_OF_STACK_RESOURCES: usize = 4;
static NETWORK_STACK_RESSOURCES_CELL: static_cell::StaticCell<
    StackResources<NUMBER_OF_STACK_RESOURCES>,
> = static_cell::StaticCell::new();
static NETWORK_STACK_CELL: static_cell::StaticCell<(
    Stack<'_>,
    Runner<'_, &mut WifiDevice<'_, WifiStaDevice>>,
)> = static_cell::StaticCell::new();

static ESP_WIFI_CONTROLLER: static_cell::StaticCell<esp_wifi::EspWifiController<'_>> =
    const { static_cell::StaticCell::new() };
static ESP_WIFI_DEVICE: static_cell::StaticCell<WifiDevice<'_, WifiStaDevice>> =
    const { static_cell::StaticCell::new() };

pub struct Hardware<'a> {
    pub stack: &'static Stack<'static>,
    pub runner: &'static mut Runner<'static, &'static mut WifiDevice<'static, WifiStaDevice>>,
    pub controller: WifiController<'static>,
    pub pmu: Pmu<I2c<'a,Blocking>>,
}

pub fn get_hardware() -> Hardware<'static> {
    const MINIMAL_HEAP_REQUIRED: usize = 72 * 1024;
    esp_alloc::heap_allocator!(MINIMAL_HEAP_REQUIRED);

    esp_println::logger::init_logger_from_env();

    let peripherals = esp_hal::init({
        let mut config = esp_hal::Config::default();
        config.cpu_clock = CpuClock::max();
        config
    });

    // PMU
    let pmu_scl = peripherals.GPIO39;
    let pmu_sda = peripherals.GPIO38;
    let config = esp_hal::i2c::master::Config::default();
    let pmu_i2c = match I2c::<Blocking>::new(peripherals.I2C0, config) {
        Ok(v) => v,
        _ => panic!(),
    }
    .with_scl(pmu_scl)
    .with_sda(pmu_sda);

    // Wifi
    let wifi_controller_timer = TimerGroup::new(peripherals.TIMG0).timer0;
    let mut random_number_generator = Rng::new(peripherals.RNG);
    let radio_clock = peripherals.RADIO_CLK;
    let wifi = peripherals.WIFI;
    let timg1_0 = TimerGroup::new(peripherals.TIMG1).timer0;

    esp_hal_embassy::init(timg1_0);

    let (wifi_device_tmp, controller) = match esp_wifi::wifi::new_with_mode(
        ESP_WIFI_CONTROLLER.init_with(|| {
            esp_wifi::init(wifi_controller_timer, random_number_generator, radio_clock).unwrap()
        }),
        wifi,
        WifiStaDevice,
    ) {
        Ok(v) => v,
        Err(_e) => panic!("Could not retrieve a wifi device"),
    };
    let wifi_device = ESP_WIFI_DEVICE.uninit().write(wifi_device_tmp);

    let seed =
        (random_number_generator.random() as u64) << 32 | (random_number_generator.random() as u64);

    let (stack, runner) = NETWORK_STACK_CELL.init_with(|| {
        new(
            wifi_device,
            embassy_net::Config::dhcpv4(DhcpConfig::default()),
            NETWORK_STACK_RESSOURCES_CELL
                .init_with(StackResources::<NUMBER_OF_STACK_RESOURCES>::new),
            // TODO : Generate random
            seed,
        )
    });
    Hardware {
        stack,
        runner,
        controller,
        pmu: Pmu::new(pmu_i2c, 0x34),
    }
}
