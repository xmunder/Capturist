use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc};
use windows::{
    core::PWSTR,
    Win32::{
        Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
        Foundation::RPC_E_CHANGED_MODE,
        Media::Audio::{
            eCapture, eConsole, EDataFlow, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
            DEVICE_STATE_ACTIVE,
        },
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
            COINIT_MULTITHREADED, STGM_READ,
        },
    },
};

#[derive(Clone)]
pub(super) struct DeviceDescriptor {
    pub(super) id: String,
    pub(super) name: String,
}

pub(super) fn list_microphone_input_devices_impl() -> Result<Vec<String>, String> {
    let mut devices = with_com(|| {
        let list = enumerate_active_devices(eCapture)?;
        Ok(list.into_iter().map(|d| d.name).collect::<Vec<_>>())
    })?;

    devices.sort_by_key(|name| name.to_lowercase());
    devices.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    Ok(devices)
}

pub(super) fn resolve_device(
    dataflow: EDataFlow,
    preferred_name: Option<&str>,
    source_label: &str,
) -> Result<DeviceDescriptor, String> {
    with_com(|| {
        let enumerator = create_device_enumerator()?;
        if let Some(name) = preferred_name.map(|s| s.trim()).filter(|s| !s.is_empty()) {
            let devices = enumerate_active_devices_from(&enumerator, dataflow)?;
            if let Some(found) = find_device_by_name(&devices, name) {
                return Ok(found);
            }

            let device_names = devices
                .iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!(
                "No se encontró un dispositivo para {} llamado '{}'. Dispositivos detectados: {}",
                source_label,
                name,
                if device_names.is_empty() {
                    "(ninguno)".to_string()
                } else {
                    device_names
                }
            ));
        }

        let default_device = unsafe {
            enumerator
                .GetDefaultAudioEndpoint(dataflow, eConsole)
                .map_err(|e| {
                    format!(
                        "No se pudo obtener endpoint WASAPI por defecto para {}: {}",
                        source_label, e
                    )
                })?
        };

        Ok(DeviceDescriptor {
            id: device_id(&default_device)?,
            name: device_friendly_name(&default_device)?,
        })
    })
}

fn with_com<T>(task: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let should_uninitialize = hr.is_ok();
    if hr.is_err() && hr != RPC_E_CHANGED_MODE {
        return Err(format!("No se pudo inicializar COM: 0x{:08X}", hr.0 as u32));
    }

    let result = task();
    if should_uninitialize {
        unsafe { CoUninitialize() };
    }
    result
}

fn create_device_enumerator() -> Result<IMMDeviceEnumerator, String> {
    unsafe {
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
            .map_err(|e| format!("No se pudo crear IMMDeviceEnumerator: {}", e))
    }
}

fn enumerate_active_devices(dataflow: EDataFlow) -> Result<Vec<DeviceDescriptor>, String> {
    let enumerator = create_device_enumerator()?;
    enumerate_active_devices_from(&enumerator, dataflow)
}

fn enumerate_active_devices_from(
    enumerator: &IMMDeviceEnumerator,
    dataflow: EDataFlow,
) -> Result<Vec<DeviceDescriptor>, String> {
    let collection = unsafe {
        enumerator
            .EnumAudioEndpoints(dataflow, DEVICE_STATE_ACTIVE)
            .map_err(|e| format!("No se pudieron listar endpoints WASAPI activos: {}", e))?
    };

    let count = unsafe {
        collection
            .GetCount()
            .map_err(|e| format!("No se pudo obtener el total de endpoints WASAPI: {}", e))?
    };

    let mut devices = Vec::with_capacity(count as usize);
    for idx in 0..count {
        let endpoint = unsafe {
            collection
                .Item(idx)
                .map_err(|e| format!("No se pudo acceder al endpoint WASAPI #{idx}: {e}"))?
        };

        devices.push(DeviceDescriptor {
            id: device_id(&endpoint)?,
            name: device_friendly_name(&endpoint)?,
        });
    }

    Ok(devices)
}

fn find_device_by_name(devices: &[DeviceDescriptor], name: &str) -> Option<DeviceDescriptor> {
    let exact = devices
        .iter()
        .find(|d| d.name.eq_ignore_ascii_case(name))
        .cloned();
    if exact.is_some() {
        return exact;
    }

    let needle = name.to_lowercase();
    devices
        .iter()
        .find(|d| d.name.to_lowercase().contains(&needle))
        .cloned()
}

fn device_id(device: &IMMDevice) -> Result<String, String> {
    let ptr = unsafe {
        device
            .GetId()
            .map_err(|e| format!("No se pudo obtener el ID del endpoint WASAPI: {}", e))?
    };
    pwstr_to_string_and_free(ptr, "ID del endpoint")
}

fn device_friendly_name(device: &IMMDevice) -> Result<String, String> {
    let store = unsafe {
        device
            .OpenPropertyStore(STGM_READ)
            .map_err(|e| format!("No se pudo abrir IPropertyStore del endpoint WASAPI: {}", e))?
    };

    let mut value = unsafe {
        store
            .GetValue(&PKEY_Device_FriendlyName)
            .map_err(|e| format!("No se pudo leer nombre del dispositivo de audio: {}", e))?
    };

    let name_result = unsafe { PropVariantToStringAlloc(&value) }.map_err(|e| {
        format!(
            "No se pudo convertir nombre del dispositivo de audio: {}",
            e
        )
    });

    let _ = unsafe { PropVariantClear(&mut value) };

    let name_ptr = name_result?;
    let name = pwstr_to_string_and_free(name_ptr, "nombre del dispositivo")?;
    if name.trim().is_empty() {
        Ok("Dispositivo sin nombre".to_string())
    } else {
        Ok(name)
    }
}

fn pwstr_to_string_and_free(ptr: PWSTR, what: &str) -> Result<String, String> {
    let value = unsafe {
        ptr.to_string()
            .map_err(|e| format!("No se pudo decodificar {} en UTF-16: {}", what, e))?
    };
    unsafe { CoTaskMemFree(Some(ptr.0 as _)) };
    Ok(value)
}

pub(super) fn to_utf16_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
