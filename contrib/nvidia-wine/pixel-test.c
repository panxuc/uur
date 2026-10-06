/* SPDX-License-Identifier: MIT
 * Repeat changing asynchronous CUDA output across unaligned D3D11 row pitches.
 * Build with MinGW-w64; run in the managed prefix while the opt-in is active.
 */
#define INITGUID
#define COBJMACROS
#include <windows.h>
#include <d3d11.h>
#include <stdio.h>
#include <string.h>

/* CUDA_MEMCPY2D_v2 ABI; no CUDA toolkit required by this dynamic-link test. */
struct copy2d {
    size_t sx, sy; int st; const void *sh; unsigned long long sd; void *sa; size_t sp;
    size_t dx, dy; int dt; void *dh; unsigned long long dd; void *da; size_t dp;
    size_t w, h;
};
#define LOAD(n, t, args) t (WINAPI *n) args = (void *)GetProcAddress(lib, #n); \
    if (!n) { puts("missing " #n); return 2; }
#define CHECK(x) do { long err = (x); if (err) { \
    printf("FAILED %s: %lx\n", #x, err); return 3; } } while (0)

int main(void)
{
    HMODULE lib = LoadLibraryA("nvcuda.dll");
    if (!lib) { printf("LoadLibrary: %lu\n", GetLastError()); return 1; }
    LOAD(cuInit, int, (unsigned));
    LOAD(cuDeviceGet, int, (int *, int));
    LOAD(cuCtxCreate_v2, int, (void **, unsigned, int));
    LOAD(cuGraphicsD3D11RegisterResource, int, (void **, ID3D11Resource *, unsigned));
    LOAD(cuGraphicsMapResources, int, (unsigned, void **, void *));
    LOAD(cuGraphicsSubResourceGetMappedArray, int, (void **, void *, unsigned, unsigned));
    LOAD(cuMemcpy2DAsync_v2, int, (struct copy2d *, void *));
    LOAD(cuGraphicsUnmapResources, int, (unsigned, void **, void *));
    LOAD(cuGraphicsUnregisterResource, int, (void *));
    LOAD(cuGraphicsResourceSetMapFlags, int, (void *, unsigned));
    LOAD(cuCtxDestroy_v2, int, (void *));
    LOAD(cuMemHostAlloc, int, (void **, size_t, unsigned));
    LOAD(cuMemFreeHost, int, (void *));
    LOAD(cuStreamCreate, int, (void **, unsigned));
    LOAD(cuStreamDestroy_v2, int, (void *));
    int dev;
    void *cuda, *stream;
    CHECK(cuInit(0)); CHECK(cuDeviceGet(&dev, 0));
    CHECK(cuCtxCreate_v2(&cuda, 0, dev)); CHECK(cuStreamCreate(&stream, 1));
    ID3D11Device *device;
    ID3D11DeviceContext *context;
    CHECK(D3D11CreateDevice(NULL, D3D_DRIVER_TYPE_HARDWARE, NULL, 0, NULL, 0,
                           D3D11_SDK_VERSION, &device, NULL, &context));
    const DXGI_FORMAT formats[] = { DXGI_FORMAT_R8_UNORM, DXGI_FORMAT_R8G8_UNORM,
                                   DXGI_FORMAT_R16_UNORM, DXGI_FORMAT_R16G16_UNORM };
    const unsigned widths[] = { 67, 35, 71, 81 }, heights[] = { 33, 19, 27, 31 };
    const unsigned bytes[] = { 1, 2, 2, 4 };
    ID3D11Texture2D *textures[4], *readback[4];
    void *resources[4], *arrays[4], *host[4];
    unsigned pitches[4];
    for (unsigned f = 0; f < 4; ++f)
    {
        D3D11_TEXTURE2D_DESC desc = {0};
        desc.Width = widths[f]; desc.Height = heights[f];
        desc.MipLevels = desc.ArraySize = desc.SampleDesc.Count = 1;
        desc.Format = formats[f]; desc.BindFlags = D3D11_BIND_SHADER_RESOURCE;
        CHECK(ID3D11Device_CreateTexture2D(device, &desc, NULL, &textures[f]));
        CHECK(cuGraphicsD3D11RegisterResource(&resources[f], (ID3D11Resource *)textures[f], 0));
        pitches[f] = widths[f] * bytes[f];
        CHECK(cuMemHostAlloc(&host[f], pitches[f] * heights[f], 0));
        desc.Usage = D3D11_USAGE_STAGING; desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
        CHECK(ID3D11Device_CreateTexture2D(device, &desc, NULL, &readback[f]));
    }
    void *mixed[] = { resources[0], (void *)1 }, *duplicate[] = { resources[0], resources[0] };
    if (cuGraphicsMapResources(2, mixed, stream) != 1 ||
        cuGraphicsUnmapResources(2, mixed, stream) != 1 ||
        cuGraphicsMapResources(2, duplicate, stream) != 1 ||
        cuGraphicsResourceSetMapFlags(resources[0], 1) != 801 ||
        cuGraphicsSubResourceGetMappedArray(&arrays[0], resources[0], 1, 0) != 1)
    { puts("FAILED unsupported/mixed-resource guard"); return 5; }
    for (unsigned frame = 0; frame < 120; ++frame)
    {
        CHECK(cuGraphicsMapResources(4, resources, stream));
        for (unsigned f = 0; f < 4; ++f)
        {
            CHECK(cuGraphicsSubResourceGetMappedArray(&arrays[f], resources[f], 0, 0));
            for (unsigned i = 0; i < pitches[f] * heights[f]; ++i)
                ((unsigned char *)host[f])[i] = (i * 13 + f * 7 + frame * 23) & 255;
            struct copy2d copy = {0};
            copy.st = 1; copy.sh = host[f]; copy.sp = pitches[f];
            copy.dt = 3; copy.da = arrays[f]; copy.w = pitches[f]; copy.h = heights[f];
            CHECK(cuMemcpy2DAsync_v2(&copy, stream));
        }
        CHECK(cuGraphicsUnmapResources(4, resources, stream));
        for (unsigned f = 0; f < 4; ++f)
        {
            ID3D11DeviceContext_CopyResource(context, (ID3D11Resource *)readback[f],
                                            (ID3D11Resource *)textures[f]);
            D3D11_MAPPED_SUBRESOURCE mapped;
            CHECK(ID3D11DeviceContext_Map(context, (ID3D11Resource *)readback[f],
                                         0, D3D11_MAP_READ, 0, &mapped));
            for (unsigned y = 0; y < heights[f]; ++y)
                if (memcmp((char *)host[f] + y * pitches[f],
                           (char *)mapped.pData + y * mapped.RowPitch, pitches[f]))
                { printf("PIXEL MISMATCH frame=%u format=%u row=%u\n", frame, formats[f], y); return 4; }
            ID3D11DeviceContext_Unmap(context, (ID3D11Resource *)readback[f], 0);
        }
    }
    for (unsigned f = 0; f < 4; ++f)
    {
        CHECK(cuGraphicsUnregisterResource(resources[f])); CHECK(cuMemFreeHost(host[f]));
        ID3D11Texture2D_Release(readback[f]); ID3D11Texture2D_Release(textures[f]);
    }
    CHECK(cuStreamDestroy_v2(stream));
    ID3D11DeviceContext_Release(context); ID3D11Device_Release(device);
    CHECK(cuCtxDestroy_v2(cuda));
    puts("PASS: 120 changing frames, R8/RG8/R16/RG16, nonblocking stream, unaligned pitches, unsupported/mixed guards");
    return 0;
}
