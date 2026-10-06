/* SPDX-License-Identifier: MIT
 * Offscreen CUDA-array -> D3D11 benchmark; not remote/input-to-display latency.
 * Every frame is read back and checked after the timed producer fence.
 */
#define INITGUID
#define COBJMACROS
#include <windows.h>
#include <d3d11.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

struct copy2d {
    size_t sx, sy; int st; const void *sh; unsigned long long sd; void *sa; size_t sp;
    size_t dx, dy; int dt; void *dh; unsigned long long dd; void *da; size_t dp;
    size_t w, h;
};
#define LOAD(n, t, args) t (WINAPI *n) args = (void *)GetProcAddress(lib, #n); \
    if (!n) { puts("missing " #n); return 2; }
#define CHECK(x) do { long err = (x); if (err) { \
    printf("FAILED %s: %lx\n", #x, err); return 3; } } while (0)

static double now_ms(void)
{
    LARGE_INTEGER counter, frequency;
    QueryPerformanceCounter(&counter); QueryPerformanceFrequency(&frequency);
    return (double)counter.QuadPart * 1000.0 / frequency.QuadPart;
}
static int compare_double(const void *a, const void *b)
{
    double x = *(const double *)a, y = *(const double *)b;
    return (x > y) - (x < y);
}
static void pace(double deadline)
{
    while (now_ms() < deadline)
        if (deadline - now_ms() > 2.0) Sleep(1);
}

int main(int argc, char **argv)
{
    const char *layout = argc > 1 ? argv[1] : "NV12";
    unsigned fps = argc > 2 ? (unsigned)strtoul(argv[2], NULL, 10) : 144;
    const unsigned warmup = 60, frames = 360;
    unsigned planes = !strcmp(layout, "NV12") ? 2 : !strcmp(layout, "R8") ? 1 : 0;
    if (!planes || fps > 1000) { puts("usage: benchmark.exe R8|NV12 [fps; 0=unpaced]"); return 1; }
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
    LOAD(cuCtxDestroy_v2, int, (void *));
    LOAD(cuMemHostAlloc, int, (void **, size_t, unsigned));
    LOAD(cuMemFreeHost, int, (void *));
    LOAD(cuStreamCreate, int, (void **, unsigned));
    LOAD(cuStreamSynchronize, int, (void *));
    LOAD(cuStreamDestroy_v2, int, (void *));
    int dev;
    void *cuda, *stream;
    CHECK(cuInit(0)); CHECK(cuDeviceGet(&dev, 0));
    CHECK(cuCtxCreate_v2(&cuda, 0, dev)); CHECK(cuStreamCreate(&stream, 1));
    ID3D11Device *device;
    ID3D11DeviceContext *context;
    CHECK(D3D11CreateDevice(NULL, D3D_DRIVER_TYPE_HARDWARE, NULL, 0, NULL, 0,
                           D3D11_SDK_VERSION, &device, NULL, &context));
    D3D11_QUERY_DESC query_desc = { D3D11_QUERY_EVENT, 0 };
    ID3D11Query *query;
    CHECK(ID3D11Device_CreateQuery(device, &query_desc, &query));
    const DXGI_FORMAT formats[] = { DXGI_FORMAT_R8_UNORM, DXGI_FORMAT_R8G8_UNORM };
    const unsigned widths[] = { 3456, 1728 }, heights[] = { 1440, 720 };
    ID3D11Texture2D *textures[2], *readback[2];
    void *resources[2], *arrays[2], *host[2];
    unsigned pitches[2];
    for (unsigned f = 0; f < planes; ++f)
    {
        D3D11_TEXTURE2D_DESC desc = {0};
        desc.Width = widths[f]; desc.Height = heights[f];
        desc.MipLevels = desc.ArraySize = desc.SampleDesc.Count = 1;
        desc.Format = formats[f]; desc.BindFlags = D3D11_BIND_SHADER_RESOURCE;
        CHECK(ID3D11Device_CreateTexture2D(device, &desc, NULL, &textures[f]));
        CHECK(cuGraphicsD3D11RegisterResource(&resources[f], (ID3D11Resource *)textures[f], 0));
        pitches[f] = widths[f] * (f ? 2 : 1);
        CHECK(cuMemHostAlloc(&host[f], (size_t)pitches[f] * heights[f], 0));
        desc.Usage = D3D11_USAGE_STAGING; desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ;
        CHECK(ID3D11Device_CreateTexture2D(device, &desc, NULL, &readback[f]));
    }
    double bridge[360], ready[360], total_bridge = 0, total_ready = 0;
    unsigned late_frames = 0;
    double start = now_ms(), measured_start = start;
    for (unsigned frame = 0; frame < warmup + frames; ++frame)
    {
        double deadline = start + (fps ? frame * 1000.0 / fps : 0);
        if (fps) pace(deadline);
        if (frame == warmup) measured_start = now_ms();
        if (frame >= warmup && fps && now_ms() > deadline + 1000.0 / fps) ++late_frames;
        CHECK(cuGraphicsMapResources(planes, resources, stream));
        for (unsigned f = 0; f < planes; ++f)
        {
            CHECK(cuGraphicsSubResourceGetMappedArray(&arrays[f], resources[f], 0, 0));
            for (unsigned y = 0; y < heights[f]; ++y)
                memset((char *)host[f] + (size_t)y * pitches[f],
                       (frame * 23 + y * 13 + f * 7) & 255, pitches[f]);
            struct copy2d copy = {0};
            copy.st = 1; copy.sh = host[f]; copy.sp = pitches[f];
            copy.dt = 3; copy.da = arrays[f]; copy.w = pitches[f]; copy.h = heights[f];
            CHECK(cuMemcpy2DAsync_v2(&copy, stream));
        }
        /* Exclude the synthetic producer's H->GPU writes from both timings. */
        CHECK(cuStreamSynchronize(stream));
        double t0 = now_ms();
        CHECK(cuGraphicsUnmapResources(planes, resources, stream));
        double t1 = now_ms();
        ID3D11DeviceContext_End(context, (ID3D11Asynchronous *)query);
        BOOL done = FALSE;
        HRESULT status;
        do {
            status = ID3D11DeviceContext_GetData(context, (ID3D11Asynchronous *)query,
                                                &done, sizeof(done), 0);
            if (FAILED(status)) CHECK(status);
            if (now_ms() - t0 > 5000) { puts("FAILED producer fence timeout"); return 6; }
        } while (status == S_FALSE || !done);
        double t2 = now_ms();
        if (frame >= warmup)
        {
            unsigned i = frame - warmup;
            bridge[i] = t1 - t0; ready[i] = t2 - t0;
            total_bridge += bridge[i]; total_ready += ready[i];
        }
        /* Consumer readback and every-byte verification are outside timing. */
        for (unsigned f = 0; f < planes; ++f)
        {
            ID3D11DeviceContext_CopyResource(context, (ID3D11Resource *)readback[f],
                                            (ID3D11Resource *)textures[f]);
            D3D11_MAPPED_SUBRESOURCE mapped;
            CHECK(ID3D11DeviceContext_Map(context, (ID3D11Resource *)readback[f],
                                         0, D3D11_MAP_READ, 0, &mapped));
            for (unsigned y = 0; y < heights[f]; ++y)
                if (memcmp((char *)host[f] + (size_t)y * pitches[f],
                           (char *)mapped.pData + (size_t)y * mapped.RowPitch, pitches[f]))
                { printf("PIXEL MISMATCH frame=%u plane=%u row=%u\n", frame, f, y); return 4; }
            ID3D11DeviceContext_Unmap(context, (ID3D11Resource *)readback[f], 0);
        }
    }
    double measured_seconds = (now_ms() - measured_start) / 1000.0;
    qsort(bridge, frames, sizeof(double), compare_double);
    qsort(ready, frames, sizeof(double), compare_double);
    printf("{\"pass\":true,\"layout\":\"%s\",\"width\":3456,\"height\":1440,"
           "\"planes\":%u,\"warmup\":%u,\"frames\":%u,\"target_fps\":%u,"
           "\"fixture_fps\":%.3f,\"late_frames\":%u,\"bridge_mean_ms\":%.6f,"
           "\"bridge_median_ms\":%.6f,\"bridge_p95_ms\":%.6f,\"bridge_max_ms\":%.6f,"
           "\"ready_mean_ms\":%.6f,\"ready_median_ms\":%.6f,\"ready_p95_ms\":%.6f,"
           "\"ready_max_ms\":%.6f}\n", layout, planes, warmup, frames, fps,
           frames / measured_seconds, late_frames, total_bridge / frames,
           bridge[frames/2], bridge[(frames*95)/100], bridge[frames-1],
           total_ready / frames, ready[frames/2], ready[(frames*95)/100], ready[frames-1]);
    for (unsigned f = 0; f < planes; ++f)
    {
        CHECK(cuGraphicsUnregisterResource(resources[f])); CHECK(cuMemFreeHost(host[f]));
        ID3D11Texture2D_Release(readback[f]); ID3D11Texture2D_Release(textures[f]);
    }
    CHECK(cuStreamDestroy_v2(stream));
    ID3D11Query_Release(query);
    ID3D11DeviceContext_Release(context); ID3D11Device_Release(device);
    CHECK(cuCtxDestroy_v2(cuda));
    return 0;
}
