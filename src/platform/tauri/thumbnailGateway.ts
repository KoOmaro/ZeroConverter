import { transportInvoke } from "./ipc";
import { toAssetUrl } from "./assetGateway";

export interface ThumbnailRequestDto {
  path: string;
  size: number;
  cache_key_hint?: string | null;
}

export interface ThumbnailResultDto {
  cached_path: string;
  width: number;
  height: number;
  cache_hit: boolean;
}

export interface ThumbnailAssetResult {
  asset_url: string;
  width: number;
  height: number;
  cache_hit: boolean;
}

export const thumbnailGateway = {
  async getOrCreateThumbnailAsset(
    path: string,
    size: number,
    cacheKeyHint?: string | null
  ): Promise<ThumbnailAssetResult> {
    const request: ThumbnailRequestDto = {
      path,
      size,
      cache_key_hint: cacheKeyHint,
    };

    const result = await transportInvoke<ThumbnailResultDto>("get_or_create_thumbnail", {
      request,
    });

    return {
      asset_url: toAssetUrl(result.cached_path),
      width: result.width,
      height: result.height,
      cache_hit: result.cache_hit,
    };
  },
};
