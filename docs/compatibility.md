# Compatibility

| Platform | Preview status |
| --- | --- |
| Windows 11 x64 | Tested on physical hardware with a standard user account |
| Ubuntu 24.04 LTS x86_64 | Linux target with passing cross-checks. Native Ubuntu compatibility has yet to be verified. |

Nucelvo reads system, CPU, memory, motherboard, graphics, and storage information through the operating system. It monitors CPU and memory usage and shows additional sensor readings when the system exposes them. Hardware, firmware, drivers, and permissions determine which readings are available.

| Reading | Windows | Linux |
| --- | --- | --- |
| CPU and memory usage | Native Windows counters | Kernel counters |
| Temperature | Firmware ACPI zones where exposed and compatible NVIDIA GPUs through their graphics driver | Linux hardware monitoring interfaces |
| GPU usage, VRAM used, fan speed, and power | Compatible NVIDIA GPUs through their graphics driver | Readings exposed by Linux hardware monitoring interfaces |
| Power source, system battery, charge, and charging status | Windows power status, aggregated by the operating system | System power supplies and individual batteries exposed by the kernel |
| Drive health | Native NVMe health logs and ATA SMART where the Windows storage driver exposes them | Optional smartmontools integration when `smartctl` is installed and permitted to read the device |

Windows NVIDIA readings were verified on a GeForce GTX 1060 6GB. The driver supplies GPU fan speed as a percentage of its maximum and power in watts. An ACPI thermal zone represents a firmware-defined part of the system. A processor package temperature requires a source that identifies the processor package.

The **System** screen captures power and battery information with the inventory. Select **Refresh inventory** to take a new reading. Charge and charging status appear when a system battery is detected.

The **Storage** screen records health readings when inventory is captured. Power-on hours are cumulative. SSD endurance used is the manufacturer's estimate of rated endurance consumed. It can exceed 100% and does not predict a failure date. An absent alert only means that the device did not report a warning in that capture.

Windows health readings were verified on the WD Black SN750 SE NVMe and the Seagate ST1000DX002-2DV162 and Toshiba MK5065GSX SATA hard drives. ATA hours, temperature, and sector counts on Windows use model-specific decoding for those two SATA models. Other ATA models may expose a failure alert, and unsupported or restricted drives show unavailable readings. USB bridges and RAID controllers may hide health data.

On Linux, the application uses `smartctl` in common system locations when present. Linux health readings require OS access to the drive. A sleeping drive may omit readings. The helper is supplied separately by the system.

The **Sensors** screen shows the state of each reading. A gap in the chart indicates a period without readings. Report formats and source-build instructions are listed in the [README](../README.md).
