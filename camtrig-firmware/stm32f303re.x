MEMORY
{
  FLASH : ORIGIN = 0x08000000, LENGTH = 512K

  /* .bss, .data and the heap go in this region */
  RAM : ORIGIN = 0x20000000, LENGTH = 64K

  /* Core coupled (faster) RAM dedicated to hold the stack */
  CCRAM : ORIGIN = 0x10000000, LENGTH = 16K
}

_stack_start = ORIGIN(CCRAM) + LENGTH(CCRAM);
