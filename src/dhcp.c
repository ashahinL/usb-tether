#include "dhcp.h"
#include "log.h"

#include <string.h>
#include <netinet/in.h>
#include <netinet/ip.h>
#include <arpa/inet.h>

#define TAG "dhcp"

int dhcp_parse_reply(const uint8_t *frame, size_t len, uint32_t xid, dhcp_reply_t *reply)
{
    const size_t ip_start = sizeof(eth_hdr_t);

    if (len < ip_start + 20)
        return -1;

    const eth_hdr_t *eth = (const eth_hdr_t *)frame;
    if (ntohs(eth->ethertype) != 0x0800)
        return -1;

    const uint8_t *ip = frame + ip_start;
    size_t ip_hdr_len = (size_t)(ip[0] & 0x0F) * 4;
    if (ip_hdr_len < 20 || ip[9] != IPPROTO_UDP)
        return -1;

    size_t udp_start = ip_start + ip_hdr_len;
    size_t dhcp_start = udp_start + 8;
    if (len < dhcp_start + sizeof(dhcp_packet_t))
        return -1;

    uint16_t dport;
    memcpy(&dport, frame + udp_start + 2, 2);
    if (ntohs(dport) != DHCP_CLIENT_PORT)
        return -1;

    const dhcp_packet_t *dhcp = (const dhcp_packet_t *)(frame + dhcp_start);
    if (ntohl(dhcp->magic) != DHCP_MAGIC_COOKIE || ntohl(dhcp->xid) != xid)
        return -1;

    memset(reply, 0, sizeof(*reply));
    reply->yiaddr = dhcp->yiaddr;

    const uint8_t *opts = dhcp->options;
    size_t opts_len = len - dhcp_start - sizeof(dhcp_packet_t);
    size_t i = 0;
    while (i < opts_len) {
        uint8_t opt = opts[i++];
        if (opt == 0xFF) break;
        if (opt == 0x00) continue;
        if (i >= opts_len) break;
        uint8_t olen = opts[i++];
        if (olen > opts_len - i) break;

        const uint8_t *val = opts + i;
        switch (opt) {
        case 1:
            if (olen >= 4) memcpy(&reply->netmask, val, 4);
            break;
        case 3:
            if (olen >= 4) memcpy(&reply->router, val, 4);
            break;
        case 6:
            if (olen >= 4) memcpy(&reply->dns1, val, 4);
            if (olen >= 8) memcpy(&reply->dns2, val + 4, 4);
            break;
        case 53:
            if (olen >= 1) reply->msg_type = val[0];
            break;
        case 54:
            if (olen >= 4) memcpy(&reply->server_id, val, 4);
            break;
        }
        i += olen;
    }

    return 0;
}

typedef struct {
    uint32_t xid;
    int want_type;
    int found;
    dhcp_reply_t reply;
} reply_wait_t;

static void match_reply(const uint8_t *frame, size_t len, void *ctx)
{
    reply_wait_t *wait = ctx;
    dhcp_reply_t reply;

    if (wait->found)
        return;
    if (dhcp_parse_reply(frame, len, wait->xid, &reply) == 0 && reply.msg_type == wait->want_type) {
        wait->reply = reply;
        wait->found = 1;
    }
}

static int wait_for_reply(usb_device_t *usb, uint32_t xid, int want_type, dhcp_reply_t *reply)
{
    uint8_t buf[RNDIS_BUF_SIZE];
    reply_wait_t wait = { .xid = xid, .want_type = want_type };

    for (int attempt = 0; attempt < 50 && !wait.found; attempt++) {
        int ret = usb_recv_bulk(usb, buf, sizeof(buf), 200);
        if (ret > 0)
            rndis_for_each_frame(buf, (size_t)ret, match_reply, &wait);
    }

    if (!wait.found)
        return -1;
    *reply = wait.reply;
    return 0;
}

static void copy_addr(char *dst, size_t dst_size, uint32_t addr, const char *fallback)
{
    struct in_addr in = { .s_addr = addr };
    strlcpy(dst, addr ? inet_ntoa(in) : fallback, dst_size);
}

int dhcp_build_discover(uint8_t *buf, size_t buf_size,
                        const uint8_t *mac, uint32_t xid)
{
    if (buf_size < 300)
        return -1;

    memset(buf, 0, buf_size);

    /* Ethernet header */
    eth_hdr_t *eth = (eth_hdr_t *)buf;
    memset(eth->dst, 0xFF, 6);
    memcpy(eth->src, mac, 6);
    eth->ethertype = htons(0x0800);

    /* IP header */
    struct ip *iph = (struct ip *)(buf + sizeof(eth_hdr_t));
    iph->ip_v = 4;
    iph->ip_hl = 5;
    iph->ip_tos = 0;
    iph->ip_id = 0;
    iph->ip_off = 0;
    iph->ip_ttl = 64;
    iph->ip_p = IPPROTO_UDP;
    iph->ip_src.s_addr = INADDR_ANY;
    iph->ip_dst.s_addr = INADDR_BROADCAST;

    /* UDP header (manual, minimal) */
    uint8_t *udp = (uint8_t *)iph + 20;
    uint16_t sport = htons(DHCP_CLIENT_PORT);
    uint16_t dport = htons(DHCP_SERVER_PORT);
    memcpy(udp, &sport, 2);
    memcpy(udp + 2, &dport, 2);

    /* DHCP payload */
    dhcp_packet_t *dhcp = (dhcp_packet_t *)(udp + 8);
    dhcp->op = 1; /* BOOTREQUEST */
    dhcp->htype = 1; /* Ethernet */
    dhcp->hlen = 6;
    dhcp->xid = htonl(xid);
    dhcp->flags = htons(0x8000); /* Broadcast */
    memcpy(dhcp->chaddr, mac, 6);
    dhcp->magic = htonl(DHCP_MAGIC_COOKIE);

    /* DHCP options: discover */
    uint8_t *opts = dhcp->options;
    *opts++ = 53; *opts++ = 1; *opts++ = 1; /* DHCP Discover */
    *opts++ = 55; *opts++ = 3; *opts++ = 1; *opts++ = 3; *opts++ = 6; /* Parameter Request */
    *opts++ = 0xFF; /* End */

    size_t dhcp_len = (size_t)(opts - (uint8_t *)dhcp);
    if (dhcp_len < 300 - sizeof(eth_hdr_t) - 20 - 8)
        dhcp_len = 300 - sizeof(eth_hdr_t) - 20 - 8;

    uint16_t udp_len = htons((uint16_t)(8 + dhcp_len));
    memcpy(udp + 4, &udp_len, 2);
    memset(udp + 6, 0, 2);

    uint16_t ip_total = htons((uint16_t)(20 + 8 + dhcp_len));
    iph->ip_len = ip_total;
    iph->ip_sum = 0;

    /* IP checksum */
    uint32_t sum = 0;
    uint16_t *p = (uint16_t *)iph;
    for (int i = 0; i < 10; i++)
        sum += ntohs(p[i]);
    while (sum >> 16)
        sum = (sum & 0xFFFF) + (sum >> 16);
    iph->ip_sum = htons((uint16_t)~sum);

    return (int)(sizeof(eth_hdr_t) + 20 + 8 + dhcp_len);
}

int dhcp_build_request(uint8_t *buf, size_t buf_size,
                       const uint8_t *mac, uint32_t xid,
                       uint32_t offered_ip, uint32_t server_ip)
{
    if (buf_size < 300)
        return -1;

    memset(buf, 0, buf_size);

    eth_hdr_t *eth = (eth_hdr_t *)buf;
    memset(eth->dst, 0xFF, 6);
    memcpy(eth->src, mac, 6);
    eth->ethertype = htons(0x0800);

    struct ip *iph = (struct ip *)(buf + sizeof(eth_hdr_t));
    iph->ip_v = 4;
    iph->ip_hl = 5;
    iph->ip_ttl = 64;
    iph->ip_p = IPPROTO_UDP;
    iph->ip_src.s_addr = INADDR_ANY;
    iph->ip_dst.s_addr = INADDR_BROADCAST;

    uint8_t *udp = (uint8_t *)iph + 20;
    uint16_t sport = htons(DHCP_CLIENT_PORT);
    uint16_t dport = htons(DHCP_SERVER_PORT);
    memcpy(udp, &sport, 2);
    memcpy(udp + 2, &dport, 2);

    dhcp_packet_t *dhcp = (dhcp_packet_t *)(udp + 8);
    dhcp->op = 1;
    dhcp->htype = 1;
    dhcp->hlen = 6;
    dhcp->xid = htonl(xid);
    dhcp->flags = htons(0x8000);
    memcpy(dhcp->chaddr, mac, 6);
    dhcp->magic = htonl(DHCP_MAGIC_COOKIE);

    uint8_t *opts = dhcp->options;
    *opts++ = 53; *opts++ = 1; *opts++ = 3; /* DHCP Request */
    *opts++ = 50; *opts++ = 4; memcpy(opts, &offered_ip, 4); opts += 4;
    *opts++ = 54; *opts++ = 4; memcpy(opts, &server_ip, 4); opts += 4;
    *opts++ = 55; *opts++ = 3; *opts++ = 1; *opts++ = 3; *opts++ = 6;
    *opts++ = 0xFF;

    size_t dhcp_len = (size_t)(opts - (uint8_t *)dhcp);
    if (dhcp_len < 300 - sizeof(eth_hdr_t) - 20 - 8)
        dhcp_len = 300 - sizeof(eth_hdr_t) - 20 - 8;

    uint16_t udp_len = htons((uint16_t)(8 + dhcp_len));
    memcpy(udp + 4, &udp_len, 2);
    memset(udp + 6, 0, 2);

    iph->ip_len = htons((uint16_t)(20 + 8 + dhcp_len));
    iph->ip_sum = 0;
    uint32_t sum = 0;
    uint16_t *p = (uint16_t *)iph;
    for (int i = 0; i < 10; i++)
        sum += ntohs(p[i]);
    while (sum >> 16)
        sum = (sum & 0xFFFF) + (sum >> 16);
    iph->ip_sum = htons((uint16_t)~sum);

    return (int)(sizeof(eth_hdr_t) + 20 + 8 + dhcp_len);
}

int dhcp_discover(usb_device_t *usb, rndis_state_t *rndis, dhcp_lease_t *lease)
{
    uint8_t rndis_buf[RNDIS_BUF_SIZE];
    uint8_t eth_buf[ETH_BUF_SIZE];
    uint32_t xid = 0x12345678;
    int ret, len;

    LOG_I(TAG, "sending DHCP discover...");

    /* Build and send DHCP discover */
    len = dhcp_build_discover(eth_buf, sizeof(eth_buf), rndis->mac_addr, xid);
    if (len < 0) return -1;

    int rndis_len = rndis_build_data_packet(rndis_buf, sizeof(rndis_buf), eth_buf, len);
    if (rndis_len < 0) return -1;

    ret = usb_send_bulk(usb, rndis_buf, rndis_len);
    if (ret < 0) return -1;

    dhcp_reply_t offer;
    if (wait_for_reply(usb, xid, 2, &offer) < 0) {
        LOG_E(TAG, "no offer received");
        return -1;
    }

    struct in_addr offered_addr = { .s_addr = offer.yiaddr };
    LOG_I(TAG, "sending DHCP request for %s...", inet_ntoa(offered_addr));

    len = dhcp_build_request(eth_buf, sizeof(eth_buf), rndis->mac_addr, xid,
                             offer.yiaddr, offer.server_id);
    if (len < 0) return -1;

    rndis_len = rndis_build_data_packet(rndis_buf, sizeof(rndis_buf), eth_buf, len);
    if (rndis_len < 0) return -1;

    ret = usb_send_bulk(usb, rndis_buf, rndis_len);
    if (ret < 0) return -1;

    dhcp_reply_t ack;
    if (wait_for_reply(usb, xid, 5, &ack) < 0) {
        LOG_E(TAG, "no ACK received");
        return -1;
    }

    copy_addr(lease->ip, sizeof(lease->ip), ack.yiaddr, DEFAULT_STATIC_IP);
    copy_addr(lease->gateway, sizeof(lease->gateway),
              ack.router ? ack.router : offer.router, DEFAULT_GATEWAY);
    copy_addr(lease->netmask, sizeof(lease->netmask),
              ack.netmask ? ack.netmask : offer.netmask, DEFAULT_NETMASK);
    copy_addr(lease->dns1, sizeof(lease->dns1),
              ack.dns1 ? ack.dns1 : offer.dns1, DEFAULT_DNS1);
    copy_addr(lease->dns2, sizeof(lease->dns2),
              ack.dns2 ? ack.dns2 : offer.dns2, DEFAULT_DNS2);

    LOG_I(TAG, "ACK: ip=%s gw=%s mask=%s dns=%s,%s",
          lease->ip, lease->gateway, lease->netmask, lease->dns1, lease->dns2);
    return 0;
}
