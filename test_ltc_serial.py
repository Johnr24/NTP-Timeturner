import serial
import re

# Adjust this as needed
SERIAL_PORT = "/dev/ttyACM0"
BAUD_RATE = 115200

# Case-insensitive pattern for: [LOCK] 10:00:00:00 | 25.00fps or FPS
ltc_pattern = re.compile(r"\[(LOCK|FREE)\]\s+(\d{2}:\d{2}:\d{2}:\d{2})\s+\|\s+([\d.]+fps)", re.IGNORECASE)

def main():
    print(f"🔌 Connecting to serial port: {SERIAL_PORT} @ {BAUD_RATE} baud")
    try:
        with serial.Serial(SERIAL_PORT, BAUD_RATE, timeout=1) as ser:
            print("📡 Listening for LTC messages...\n")
            while True:
                line = ser.readline().decode(errors='ignore').strip()
                match = ltc_pattern.match(line)
                if match:
                    status, timecode, framerate = match.groups()
                    framerate = framerate.upper()  # Standardise to FPS
                    if status == "LOCK":
                        print(f"🔒 {status:<4} | ⏱ {timecode} | 🎞 {framerate}")
                    else:
                        print(f"🟡 {status:<4} | ⏱ {timecode} | 🎞 {framerate}")
                else:
                    if line:
                        print(f"⚠️ Unrecognised line: {line}")
    except serial.SerialException as e:
        print(f"❌ Serial error: {e}")
    except KeyboardInterrupt:
        print("\n🛑 Stopped by user.")

if __name__ == "__main__":
    main()
