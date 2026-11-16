#include "dhcp.h"
#include "rndis.h"

#include <arpa/inet.h>
#include <stdio.h>
#include <string.h>

static int failures;

#define CHECK(cond) do { \
    if (!(cond)) { fprintf(stderr, "%s:%d: CHECK failed: %s\n", __FILE__, __LINE__, #cond); failures++; } \
} while (0)

typedef struct {
    int count;
    size_t lens[8];
} frames_t;

static void collect(const uint8_t *frame, size_t len, void *ctx)
{
    (void)frame;
    frames_t *f = ctx;
    if (f->count < 8)
        f->lens[f->count] = len;
    f->count++;
}

static size_t put_data_packet(uint8_t *buf, size_t eth_len)
{
    uint8_t eth[64];
    memset(eth, 0xAB, sizeof(eth));
    return (size_t)rndis_build_data_packet(buf, 256, eth, eth_len);
}

static void test_walk_returns_every_frame(void)
{
    uint8_t buf[512];
    size_t a = put_data_packet(buf, 60);
    size_t b = put_data_packet(buf + a, 42);
    frames_t f = {0};

    rndis_for_each_frame(buf, a + b, collect, &f);

    CHECK(f.count == 2);
    CHECK(f.lens[0] == 60);
    CHECK(f.lens[1] == 42);
}

static void test_walk_stops_at_wrapping_length(void)
{
    uint8_t buf[512];
    size_t a = put_data_packet(buf, 60);
    size_t b = put_data_packet(buf + a, 42);
    uint32_t huge = 0xFFFFFFFFu - (uint32_t)a + 1;
    memcpy(buf + a + 4, &huge, 4);
    frames_t f = {0};

    rndis_for_each_frame(buf, a + b, collect, &f);

    CHECK(f.count == 1);
}

static void test_walk_skips_payload_past_message(void)
{
    uint8_t buf[512];
    size_t a = put_data_packet(buf, 60);
    uint32_t far_offset = 0xFFFFFF00u;
    memcpy(buf + 8, &far_offset, 4);
    frames_t f = {0};

    rndis_for_each_frame(buf, a, collect, &f);

    CHECK(f.count == 0);
}

static size_t build_reply(uint8_t *buf, uint32_t xid, const uint8_t *opts, size_t opts_len)
{
    memset(buf, 0, 600);
    buf[12] = 0x08;
    buf[13] = 0x00;
    uint8_t *ip = buf + 14;
    ip[0] = 0x45;
    ip[9] = IPPROTO_UDP;
    uint8_t *udp = ip + 20;
    udp[2] = 0;
    udp[3] = DHCP_CLIENT_PORT;
    dhcp_packet_t *dhcp = (dhcp_packet_t *)(udp + 8);
    dhcp->xid = htonl(xid);
    dhcp->yiaddr = inet_addr("10.239.0.5");
    dhcp->magic = htonl(DHCP_MAGIC_COOKIE);
    memcpy(dhcp->options, opts, opts_len);
    return 14 + 20 + 8 + sizeof(dhcp_packet_t) + opts_len;
}

static void test_reply_reads_offer_fields(void)
{
    const uint8_t opts[] = {
        53, 1, 2,
        54, 4, 10, 239, 0, 1,
        1, 4, 255, 255, 255, 0,
        3, 4, 10, 239, 0, 1,
        6, 8, 8, 8, 8, 8, 1, 1, 1, 1,
        0xFF,
    };
    uint8_t frame[600];
    size_t len = build_reply(frame, 7, opts, sizeof(opts));
    dhcp_reply_t r;

    CHECK(dhcp_parse_reply(frame, len, 7, &r) == 0);
    CHECK(r.msg_type == 2);
    CHECK(r.yiaddr == inet_addr("10.239.0.5"));
    CHECK(r.server_id == inet_addr("10.239.0.1"));
    CHECK(r.router == inet_addr("10.239.0.1"));
    CHECK(r.netmask == inet_addr("255.255.255.0"));
    CHECK(r.dns1 == inet_addr("8.8.8.8"));
    CHECK(r.dns2 == inet_addr("1.1.1.1"));
}

static void test_reply_ignores_option_cut_off_by_frame_end(void)
{
    const uint8_t opts[] = { 53, 1, 5, 54, 4, 10, 239 };
    uint8_t frame[600];
    size_t len = build_reply(frame, 7, opts, sizeof(opts));
    dhcp_reply_t r;

    CHECK(dhcp_parse_reply(frame, len, 7, &r) == 0);
    CHECK(r.msg_type == 5);
    CHECK(r.server_id == 0);
}

static void test_reply_ignores_type_option_with_no_value(void)
{
    const uint8_t opts[] = { 53, 1 };
    uint8_t frame[600];
    size_t len = build_reply(frame, 7, opts, sizeof(opts));
    dhcp_reply_t r;

    CHECK(dhcp_parse_reply(frame, len, 7, &r) == 0);
    CHECK(r.msg_type == 0);
}

static void test_reply_rejects_ip_header_longer_than_frame(void)
{
    uint8_t frame[600];
    size_t len = build_reply(frame, 7, (const uint8_t[]){ 0xFF }, 1);
    frame[14] = 0x4F;

    dhcp_reply_t r;
    CHECK(dhcp_parse_reply(frame, 14 + 60, 7, &r) == -1);
    (void)len;
}

static void test_reply_rejects_other_xid(void)
{
    uint8_t frame[600];
    size_t len = build_reply(frame, 7, (const uint8_t[]){ 53, 1, 2, 0xFF }, 4);
    dhcp_reply_t r;

    CHECK(dhcp_parse_reply(frame, len, 8, &r) == -1);
}

int main(void)
{
    test_walk_returns_every_frame();
    test_walk_stops_at_wrapping_length();
    test_walk_skips_payload_past_message();
    test_reply_reads_offer_fields();
    test_reply_ignores_option_cut_off_by_frame_end();
    test_reply_ignores_type_option_with_no_value();
    test_reply_rejects_ip_header_longer_than_frame();
    test_reply_rejects_other_xid();

    if (failures) {
        fprintf(stderr, "%d check(s) failed\n", failures);
        return 1;
    }
    printf("all parser tests passed\n");
    return 0;
}
