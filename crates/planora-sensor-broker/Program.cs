using System;
using System.Collections.Generic;
using System.IO;
using System.Text.Json;
using System.Text.Json.Serialization;
using LibreHardwareMonitor.Hardware;

namespace PlanoraSensorBroker;

public class BrokerReading
{
    [JsonPropertyName("id")]
    public string Id { get; set; } = string.Empty;

    [JsonPropertyName("device_id")]
    public string DeviceId { get; set; } = string.Empty;

    [JsonPropertyName("device_name")]
    public string DeviceName { get; set; } = string.Empty;

    [JsonPropertyName("name")]
    public string Name { get; set; } = string.Empty;

    [JsonPropertyName("type")]
    public string SensorType { get; set; } = string.Empty;

    [JsonPropertyName("scope")]
    public string Scope { get; set; } = string.Empty;

    [JsonPropertyName("provider")]
    public string Provider { get; set; } = "LibreHardwareMonitor";

    [JsonPropertyName("unit")]
    public string Unit { get; set; } = string.Empty;

    [JsonPropertyName("value")]
    public float? Value { get; set; }

    [JsonPropertyName("sampled_at_ms")]
    public ulong SampledAtMs { get; set; }
}

public class SampleResponse
{
    [JsonPropertyName("status")]
    public string Status { get; set; } = "ok";

    [JsonPropertyName("timestamp_ms")]
    public ulong TimestampMs { get; set; }

    [JsonPropertyName("sensors")]
    public List<BrokerReading> Sensors { get; set; } = new();
}

public class PingResponse
{
    [JsonPropertyName("status")]
    public string Status { get; set; } = "ok";

    [JsonPropertyName("version")]
    public string Version { get; set; } = "0.3.0-beta.8";

    [JsonPropertyName("lhm_version")]
    public string LhmVersion { get; set; } = "0.9.6";
}

public class UpdateVisitor : IVisitor
{
    public void VisitComputer(IComputer computer)
    {
        computer.Traverse(this);
    }

    public void VisitHardware(IHardware hardware)
    {
        hardware.Update();
        foreach (IHardware subHardware in hardware.SubHardware)
            subHardware.Accept(this);
    }

    public void VisitSensor(ISensor sensor) { }
    public void VisitParameter(IParameter parameter) { }
}

public static class Program
{
    public static void Main(string[] args)
    {
        // Enforce culture invariant for numeric conversions
        System.Threading.Thread.CurrentThread.CurrentCulture = System.Globalization.CultureInfo.InvariantCulture;

        Computer? computer = null;
        UpdateVisitor? visitor = null;

        try
        {
            // Strictly read-only operations.
            // Disable fan control, overclocking, arbitrary register access, user-supplied modules,
            // and broad embedded-controller/SMBus probing as specified in requirements.
            computer = new Computer
            {
                IsCpuEnabled = true,
                IsGpuEnabled = true,
                IsStorageEnabled = true,
                IsMotherboardEnabled = false,
                IsControllerEnabled = false,
                IsMemoryEnabled = false,
                IsNetworkEnabled = false
            };

            computer.Open();
            visitor = new UpdateVisitor();
            computer.Accept(visitor);
        }
        catch (Exception ex)
        {
            var errJson = JsonSerializer.Serialize(new
            {
                status = "error",
                message = $"Failed to initialize LibreHardwareMonitor: {ex.Message}"
            });
            Console.WriteLine(errJson);
            Console.Out.Flush();
        }

        string? line;
        while ((line = Console.ReadLine()) != null)
        {
            line = line.Trim();
            if (string.IsNullOrEmpty(line))
                continue;

            if (line.Equals("exit", StringComparison.OrdinalIgnoreCase) ||
                line.Contains("\"exit\"", StringComparison.OrdinalIgnoreCase))
            {
                break;
            }

            if (line.Contains("\"ping\"", StringComparison.OrdinalIgnoreCase))
            {
                var ping = new PingResponse();
                Console.WriteLine(JsonSerializer.Serialize(ping));
                Console.Out.Flush();
                continue;
            }

            if (computer == null || visitor == null)
            {
                Console.WriteLine(JsonSerializer.Serialize(new
                {
                    status = "error",
                    message = "Sensor broker computer is not initialized"
                }));
                Console.Out.Flush();
                continue;
            }

            try
            {
                computer.Accept(visitor);
                ulong nowMs = (ulong)DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
                var readings = new List<BrokerReading>();

                foreach (var hardware in computer.Hardware)
                {
                    CollectHardwareSensors(hardware, readings, nowMs);
                }

                var response = new SampleResponse
                {
                    Status = "ok",
                    TimestampMs = nowMs,
                    Sensors = readings
                };

                Console.WriteLine(JsonSerializer.Serialize(response));
                Console.Out.Flush();
            }
            catch (Exception ex)
            {
                Console.WriteLine(JsonSerializer.Serialize(new
                {
                    status = "error",
                    message = $"Sampling exception: {ex.Message}"
                }));
                Console.Out.Flush();
            }
        }

        try
        {
            computer?.Close();
        }
        catch
        {
            // Ignore cleanup errors on exit
        }
    }

    private static void CollectHardwareSensors(IHardware hardware, List<BrokerReading> readings, ulong nowMs)
    {
        string deviceId = hardware.Identifier.ToString().TrimStart('/');
        string deviceName = hardware.Name;

        foreach (var sensor in hardware.Sensors)
        {
            string typeStr;
            string unitStr;
            string defaultScope;

            switch (sensor.SensorType)
            {
                case SensorType.Voltage:
                    typeStr = "Voltage";
                    unitStr = "V";
                    defaultScope = "Device";
                    break;
                case SensorType.Current:
                    typeStr = "Current";
                    unitStr = "A";
                    defaultScope = "Device";
                    break;
                case SensorType.Power:
                    typeStr = "Power";
                    unitStr = "W";
                    defaultScope = sensor.Name.Contains("Package", StringComparison.OrdinalIgnoreCase) ? "Package" :
                                   sensor.Name.Contains("Core", StringComparison.OrdinalIgnoreCase) ? "Core" :
                                   sensor.Name.Contains("Board", StringComparison.OrdinalIgnoreCase) ? "Board" : "Device";
                    break;
                case SensorType.Temperature:
                    typeStr = "Temperature";
                    unitStr = "°C";
                    defaultScope = sensor.Name.Contains("Package", StringComparison.OrdinalIgnoreCase) ? "Package" :
                                   sensor.Name.Contains("Core", StringComparison.OrdinalIgnoreCase) ? "Core" :
                                   sensor.Name.Contains("Hot Spot", StringComparison.OrdinalIgnoreCase) ? "Hotspot" :
                                   hardware.HardwareType == HardwareType.Storage ? "Drive" : "Device";
                    break;
                case SensorType.Fan:
                    typeStr = "Fan";
                    unitStr = "RPM";
                    defaultScope = hardware.HardwareType is HardwareType.GpuIntel or HardwareType.GpuNvidia or HardwareType.GpuAmd ? "GPU" : "Chassis";
                    break;
                case SensorType.Clock:
                    typeStr = "Clock";
                    unitStr = "MHz";
                    defaultScope = sensor.Name.Contains("Core", StringComparison.OrdinalIgnoreCase) ? "Core" : "Device";
                    break;
                case SensorType.Load:
                    typeStr = "Load";
                    unitStr = "%";
                    defaultScope = sensor.Name.Contains("Total", StringComparison.OrdinalIgnoreCase) ? "Package" :
                                   sensor.Name.Contains("Core", StringComparison.OrdinalIgnoreCase) ? "Core" : "Device";
                    break;
                case SensorType.Energy:
                    typeStr = "Energy";
                    unitStr = "J";
                    defaultScope = "Package";
                    break;
                default:
                    // Only process supported physical categories
                    continue;
            }

            string sensorId = sensor.Identifier.ToString().TrimStart('/');
            readings.Add(new BrokerReading
            {
                Id = sensorId,
                DeviceId = deviceId,
                DeviceName = deviceName,
                Name = sensor.Name,
                SensorType = typeStr,
                Scope = defaultScope,
                Provider = "LibreHardwareMonitor",
                Unit = unitStr,
                Value = sensor.Value,
                SampledAtMs = nowMs
            });
        }

        foreach (var sub in hardware.SubHardware)
        {
            CollectHardwareSensors(sub, readings, nowMs);
        }
    }
}
