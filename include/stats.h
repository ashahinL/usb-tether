#ifndef STATS_H
#define STATS_H

#include <stdint.h>

/* Tethering session statistics */
typedef struct {
    double tx_mbps;
    double rx_mbps;
    uint64_t tx_bytes;
    uint64_t rx_bytes;
    uint64_t tx_pkts;
    uint64_t rx_pkts;
} tether_stats_t;

#endif /* STATS_H */
