#define WIN32_LEAN_AND_MEAN
#include <winsock2.h>
#include <windows.h>

#include <stdint.h>
#include <stdio.h>
#include <string.h>

#define TOKEN_BYTES 64
#define MAGIC 0x55554c41u

static int sibling(char *path, size_t capacity, const char *name)
{
    DWORD length = GetModuleFileNameA(NULL, path, (DWORD)capacity);
    char *separator;
    if (length == 0 || length >= capacity)
        return 0;
    separator = strrchr(path, '\\');
    if (separator == NULL)
        return 0;
    if ((size_t)(separator - path) + strlen(name) + 2 > capacity)
        return 0;
    strcpy(separator + 1, name);
    return 1;
}

static int read_small_file(const char *path, char *buffer, size_t capacity)
{
    HANDLE file;
    DWORD count;
    file = CreateFileA(path, GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_DELETE,
                       NULL, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
    if (file == INVALID_HANDLE_VALUE)
        return 0;
    if (!ReadFile(file, buffer, (DWORD)capacity - 1, &count, NULL)) {
        CloseHandle(file);
        return 0;
    }
    CloseHandle(file);
    buffer[count] = '\0';
    while (count > 0 && (buffer[count - 1] == '\r' || buffer[count - 1] == '\n'))
        buffer[--count] = '\0';
    return count > 0;
}

static int send_all(SOCKET socket, const void *data, size_t length)
{
    const char *cursor = data;
    while (length > 0) {
        int sent = send(socket, cursor, (int)length, 0);
        if (sent <= 0)
            return 0;
        cursor += sent;
        length -= (size_t)sent;
    }
    return 1;
}

static void put_u16_be(unsigned char *output, uint16_t value)
{
    output[0] = (unsigned char)(value >> 8);
    output[1] = (unsigned char)value;
}

static void put_u32_be(unsigned char *output, uint32_t value)
{
    output[0] = (unsigned char)(value >> 24);
    output[1] = (unsigned char)(value >> 16);
    output[2] = (unsigned char)(value >> 8);
    output[3] = (unsigned char)value;
}

int main(void)
{
    char id_path[32768];
    char app_id[512];
    char config[512];
    char token[TOKEN_BYTES + 1];
    unsigned port;
    WSADATA winsock;
    SOCKET socket = INVALID_SOCKET;
    struct sockaddr_in address;
    unsigned char header[12];
    unsigned char accepted = 0;
    size_t id_length;
    int result = 1;

    if (!sibling(id_path, sizeof(id_path), "app-id.txt") ||
        !read_small_file(id_path, app_id, sizeof(app_id)) ||
        !read_small_file("C:\\uur-launcher.runtime", config, sizeof(config)) ||
        sscanf(config, "version=1\nport=%u\ntoken=%64[0-9a-f]\n", &port, token) != 2 ||
        port == 0 || port > 65535 || strlen(token) != TOKEN_BYTES)
        return 2;
    id_length = strlen(app_id);
    if (id_length == 0 || id_length > 500 || WSAStartup(MAKEWORD(2, 2), &winsock) != 0)
        return 3;
    socket = WSASocketA(AF_INET, SOCK_STREAM, IPPROTO_TCP, NULL, 0, 0);
    if (socket == INVALID_SOCKET)
        goto done;
    ZeroMemory(&address, sizeof(address));
    address.sin_family = AF_INET;
    address.sin_port = htons((uint16_t)port);
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    if (connect(socket, (struct sockaddr *)&address, sizeof(address)) != 0)
        goto done;
    put_u32_be(header + 0, MAGIC);
    put_u16_be(header + 4, 1);
    put_u16_be(header + 6, TOKEN_BYTES);
    put_u16_be(header + 8, (uint16_t)id_length);
    put_u16_be(header + 10, 0);
    if (!send_all(socket, header, sizeof(header)) ||
        !send_all(socket, token, TOKEN_BYTES) ||
        !send_all(socket, app_id, id_length) || recv(socket, (char *)&accepted, 1, 0) != 1)
        goto done;
    result = accepted == 1 ? 0 : 4;
done:
    if (socket != INVALID_SOCKET)
        closesocket(socket);
    SecureZeroMemory(token, sizeof(token));
    WSACleanup();
    return result;
}
