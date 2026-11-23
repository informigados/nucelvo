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

Windows NVIDIA readings were verified on a GeForce GTX 1060 6GB. The driver supplies GPU fan speed as a percentage of its maximum and power in watts. An ACPI thermal zone represents a firmware-defined part of the system. A processor package temperature requires a source that identifies the processor package.

The **Sensors** screen shows the state of each reading. A gap in the chart indicates a period without readings. Report formats and source-build instructions are listed in the [README](../README.md).
