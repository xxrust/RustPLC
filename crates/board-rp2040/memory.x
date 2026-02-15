/* Minimal memory layout for RP2040 (Raspberry Pi Pico). */
MEMORY
{
  FLASH : ORIGIN = 0x10000000, LENGTH = 2048K
  RAM   : ORIGIN = 0x20000000, LENGTH = 264K
}

