/* SPDX-License-Identifier: LGPL-2.1-or-later
 * Experimental output-texture bridge for the official UU decoder.
 * Included in SveSop/nvcuda's translation unit so the normal CUDA forwarding
 * remains unchanged. This is host copyback, not CUDA/D3D zero-copy interop.
 * Only single-layer, single-mip, non-MSAA decoder output textures are supported.
 */
struct uur_texture
{
    struct uur_texture *next;
    ID3D11Resource *texture;
    ID3D11DeviceContext *context;
    ID3D11Texture2D *staging;
    CUarray array;
    UINT width, height, row_bytes;
};
static struct uur_texture *uur_textures;
static pthread_mutex_t uur_texture_mutex = PTHREAD_MUTEX_INITIALIZER;

/* The upstream minimal CUDA header makes this descriptor opaque. Keep its
 * CUDA_ARRAY_DESCRIPTOR_v2 ABI explicit, including size_t on 64-bit hosts. */
struct uur_array_descriptor { size_t width, height; unsigned format, channels; };

static struct uur_texture *uur_find(CUgraphicsResource resource)
{
    struct uur_texture *p;
    for (p = uur_textures; p; p = p->next)
        if ((CUgraphicsResource)p == resource) return p;
    return NULL;
}

/* 0: all native handles, 1: all bridge handles, -1: mixed/duplicate handles.
 * Never pass our private handles to the real driver in a mixed batch. */
static int uur_resource_batch(unsigned count, CUgraphicsResource *resources)
{
    unsigned i, j, bridges = 0;
    if (count && !resources) return -1;
    for (i = 0; i < count; ++i)
    {
        for (j = 0; j < i; ++j) if (resources[j] == resources[i]) return -1;
        if (uur_find(resources[i])) ++bridges;
    }
    return !bridges ? 0 : bridges == count ? 1 : -1;
}

static CUresult uur_upload(struct uur_texture *p, CUstream stream)
{
    D3D11_MAPPED_SUBRESOURCE mapped;
    CUDA_MEMCPY2D copy = {0};
    CUresult result;
    HRESULT hr;

    /* Honour the decoder's nonblocking stream before reading its real array. */
    result = pcuStreamSynchronize(stream);
    if (result) return result;
    hr = ID3D11DeviceContext_Map(p->context, (ID3D11Resource *)p->staging,
                               0, D3D11_MAP_WRITE, 0, &mapped);
    if (FAILED(hr)) return CUDA_ERROR_UNKNOWN;
    if (mapped.RowPitch < p->row_bytes)
    {
        ID3D11DeviceContext_Unmap(p->context, (ID3D11Resource *)p->staging, 0);
        return CUDA_ERROR_INVALID_VALUE;
    }
    copy.srcMemoryType = 3; /* CU_MEMORYTYPE_ARRAY */
    copy.srcArray = p->array;
    copy.dstMemoryType = 1; /* CU_MEMORYTYPE_HOST */
    copy.dstHost = mapped.pData;
    copy.dstPitch = mapped.RowPitch;
    copy.WidthInBytes = p->row_bytes;
    copy.Height = p->height;
    result = pcuMemcpy2D_v2(&copy);
    ID3D11DeviceContext_Unmap(p->context, (ID3D11Resource *)p->staging, 0);
    if (!result)
        ID3D11DeviceContext_CopyResource(p->context, p->texture,
                                        (ID3D11Resource *)p->staging);
    return result;
}

static CUresult uur_register(CUgraphicsResource *resource, ID3D11Resource *texture,
                             unsigned flags)
{
    D3D11_RESOURCE_DIMENSION dimension;
    D3D11_TEXTURE2D_DESC desc;
    struct uur_array_descriptor array = {0};
    struct uur_texture *p;
    ID3D11Device *device;
    unsigned channels, bytes = 1;
    CUresult result;
    HRESULT hr;

    if (!resource || !texture) return CUDA_ERROR_INVALID_VALUE;
    ID3D11Resource_GetType(texture, &dimension);
    if (dimension != D3D11_RESOURCE_DIMENSION_TEXTURE2D) return CUDA_ERROR_NOT_SUPPORTED;
    ID3D11Texture2D_GetDesc((ID3D11Texture2D *)texture, &desc);
    if (desc.ArraySize != 1 || desc.MipLevels != 1 || desc.SampleDesc.Count != 1 || flags)
        return CUDA_ERROR_NOT_SUPPORTED;
    switch (desc.Format)
    {
        case DXGI_FORMAT_R8_UNORM: channels = 1; break;
        case DXGI_FORMAT_R8G8_UNORM: channels = 2; break;
        case DXGI_FORMAT_R8G8B8A8_UNORM:
        case DXGI_FORMAT_B8G8R8A8_UNORM: channels = 4; break;
        case DXGI_FORMAT_R16_UNORM: channels = 1; bytes = 2; break;
        case DXGI_FORMAT_R16G16_UNORM: channels = 2; bytes = 2; break;
        default: return CUDA_ERROR_NOT_SUPPORTED;
    }
    if (!desc.Width || !desc.Height || desc.Width > UINT_MAX / (channels * bytes))
        return CUDA_ERROR_INVALID_VALUE;
    p = calloc(1, sizeof(*p));
    if (!p) return CUDA_ERROR_OUT_OF_MEMORY;
    p->width = desc.Width;
    p->height = desc.Height;
    p->row_bytes = desc.Width * channels * bytes;
    array.width = desc.Width;
    array.height = desc.Height;
    array.format = bytes == 1 ? 1 : 2; /* CU_AD_FORMAT_UNSIGNED_INT8 / INT16 */
    array.channels = channels;
    result = pcuArrayCreate_v2(&p->array, (const CUDA_ARRAY_DESCRIPTOR *)&array);
    if (result) { free(p); return result; }
    ID3D11Resource_GetDevice(texture, &device);
    ID3D11Device_GetImmediateContext(device, &p->context);
    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;
    desc.MiscFlags = 0;
    hr = ID3D11Device_CreateTexture2D(device, &desc, NULL, &p->staging);
    ID3D11Device_Release(device);
    if (FAILED(hr))
    {
        ID3D11DeviceContext_Release(p->context);
        pcuArrayDestroy(p->array);
        free(p);
        return CUDA_ERROR_NOT_SUPPORTED;
    }
    p->texture = texture;
    ID3D11Resource_AddRef(texture);
    pthread_mutex_lock(&uur_texture_mutex);
    p->next = uur_textures;
    uur_textures = p;
    pthread_mutex_unlock(&uur_texture_mutex);
    *resource = (CUgraphicsResource)p;
    WARN("UU NVDEC output texture: %ux%u DXGI format %u\n", p->width, p->height, desc.Format);
    return CUDA_SUCCESS;
}
