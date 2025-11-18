XDP (eXpress Data Path) is a high-performance data packet processing mechanism based on eBPF.
It mounts at the earliest entry of the network card driver (before the kernel protocol stack),
meaning that the packet has not entered the kernel protocol stack, and can be processed by the XDP program.
XDP's goal is to achieve low latency and high throughput packet processing.
XDP programs run in kernel space, so they can avoid user space overhead and access kernel data structures, such as sk_buff.

compile XDP program:
```bash
clang -O2 -target bpf -c xdp_filter.c -o xdp_filter.o
```

here we can use make command
first time compile need to use make deps install dependencies
when clean use make clean

load XDP program:
```bash
sudo ip link set dev <interface> xdp obj xdp_filter.o sec xdp
```

unload XDP program:
```bash
sudo ip link set dev <interface> xdp off
```

show XDP program
```bash
sudo bpftool prog show #or sudo ip -d link show  <interface>
```

add API key
```bash
sudo bpftool map update name api_keys key hex 31 32 33 34 35 36 37 38 39 30 30 30 30 30 30 30 value hex 01
```

precondition
check netcard driver type
```bash
ethtool -i <interface>
```