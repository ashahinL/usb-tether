#ifndef DHCP_H
#define DHCP_H

#include "net_types.h"
#include "usb_device.h"
#include "rndis.h"

#include <netinet/in.h>

/* Result of a DHCP lease */
typedef struct {
    char ip[16];
    char gateway[16];
    char netmask[16];
    char dns1[16];
    char dns2[16];
} dhcp_lease_t;

/* Perform DHCP discover/offer/request/ack sequence over USB.
   Returns 0 on success, -1 on failure. */
int dhcp_discover(usb_device_t *usb, rndis_state_t *rndis, dhcp_lease_t *lease);

/* Fields of a DHCP reply, in network byte order. 0 means absent. */
typedef struct {
    int msg_type;
    uint32_t yiaddr;
    uint32_t server_id;
    uint32_t router;
    uint32_t netmask;
    uint32_t dns1;
    uint32_t dns2;
} dhcp_reply_t;

/* Parse an Ethernet frame as a DHCP reply to xid. Returns 0 on success. */
int dhcp_parse_reply(const uint8_t *frame, size_t len, uint32_t xid, dhcp_reply_t *reply);

/* Build a DHCP discover packet inside an Ethernet frame.
   Returns frame length or -1 on error. */
int dhcp_build_discover(uint8_t *buf, size_t buf_size,
                        const uint8_t *mac, uint32_t xid);

/* Build a DHCP request packet inside an Ethernet frame.
   Returns frame length or -1 on error. */
int dhcp_build_request(uint8_t *buf, size_t buf_size,
                       const uint8_t *mac, uint32_t xid,
                       uint32_t offered_ip, uint32_t server_ip);

#endif /* DHCP_H */
