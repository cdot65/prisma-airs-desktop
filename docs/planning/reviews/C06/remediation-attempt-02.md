# C06 remediation after attempt-02

Verdict attempt-02: architecture 9, behavior 8, authority/security 8,
validation 8, overall 8; C06-A02-RF01, C06-A02-RF02.

## C06-A02-RF01 — uncertainty and partial effects of avatar replacement

Academy stores avatars under the per-user key `avatars/<user>.jpg`, so a
stored PUT can replace the photo already served before or without
confirmation. The core now distinguishes what each step is known to have
done (`academy::StoreOutcome`, `AvatarJob::run`):

- not sent (no connection) or refused (4xx): nothing stored, the photo is
  unchanged (`UploadFailed`, NotSent/Rejected);
- a 5xx, a lost reply, a timeout or cancellation after sending: the bytes
  may be stored (`UploadFailed`, Unknown);
- stored, then a confirmation that is not a success (refused, failed or
  lost): the photo under the existing address may already be the new one
  (Academy's reply, Unknown). Nothing is ever resent.

Academy's host-upload flow says "Your photo may have changed, but Academy
did not confirm it. The photo shown is what Academy serves now; upload again
if it is not the one you chose." for every possibly applied outcome, re-reads
the settings and reloads host-loaded images even at an unchanged address
(`refreshLearnerImages`); "Your photo is unchanged" is used only when
nothing was stored.

Tests: Rust `an_upload_that_may_have_replaced_the_photo_is_reported_as_possibly_applied`
(lost store reply via a closed connection, store 500, stored then
confirmation 404 and 503: Unknown, one PUT/confirm each, none resent),
`an_upload_cancelled_after_its_photo_was_sent_is_not_reported_as_unchanged`
(sign-out while the store holds the reply: Cancelled/Unknown, no confirm);
refused store remains Rejected. Composed: four outcome cases plus "reads the
photo Academy serves again after an upload that may have replaced it at the
same address" (same URL, new image shown). Academy: `learner-image`
"reads the same address again after a refresh". Native C06-N13: with the
store keeping the bytes and losing its reply, the page showed the
may-have-changed message, sent no confirmation and read the avatar at the
unchanged address again. Probes: lost store reply as Rejected, refused
confirmation as Rejected, removed image refresh — each fails its guard.

## C06-A02-RF02 — bounded image preparation

`crate::avatar::prepare` follows v1 (`profile_media.rs`) with the same
`image` 0.25.10 (JPEG, PNG and WebP decoders only): source photos up to
16 MiB (the host picker's read cap), format from the bytes, decoder limits
8192 pixels a side and 128 MiB, EXIF orientation applied, a 512-pixel square
filled (Lanczos3) and encoded as JPEG quality 85 within Academy's 2 MiB cap;
the upload is always `image/jpeg`/`jpg`. Anything that does not decode
completely is never presigned or uploaded. Readback decodes and re-encodes
Academy's image the same way (declared type must match the decoded format).

Tests (unit, `avatar.rs`): PNG/JPEG/WebP sources become 512x512 JPEGs within
the cap; a noisy PNG above 2 MiB is accepted and prepared; an EXIF
orientation-6 JPEG (red left, blue right) comes out red on top and blue
below; malformed files with valid signatures (text after a PNG signature,
truncated PNG and JPEG, broken WebP), GIF, SVG, empty, beyond 16 MiB and
beyond 8192 pixels are refused. Rust integration: the prepared upload is a
512-pixel JPEG with matching presign size; malformed and truncated photos
send nothing. Native C06-N13: a 5.9 MB PNG prepared as a 232 KB JPEG and
uploaded; a file with a PNG signature that does not decode sent nothing and
said the photo is unchanged. Probes: oversize source accepted, undecoded
bytes uploaded — each fails its guard.

New dependencies (notice inventory regenerated): image 0.25.10, image-webp
0.2.4, zune-jpeg 0.5.15, zune-core 0.5.3, moxcms 0.8.1, pxfm 0.1.30,
byteorder-lite 0.1.0, quick-error 2.0.1 — the versions v1 locked; licences
MIT/Apache-2.0/BSD-3-Clause/Zlib/Unlicense.

# C06 remediation after attempt-03

Verdict attempt-03: architecture 9, behavior 8, authority/security 8,
validation 8, overall 8; C06-A03-RF01.

## C06-A03-RF01 — decoded-output memory budget

`avatar::reencode` now reserves the decoder's `total_bytes()` from the
128 MiB budget before any pixel is allocated and hands the decoder only the
remaining budget (`decoder.set_limits`); `image` 0.25.10's `into_decoder`
forwards limits but, unlike `ImageReader::decode`, does not reserve the
output buffer. Orientation is applied to the 512-pixel square instead of the
full image (a centred square fill commutes with every EXIF rotation and
flip), so no second full-size buffer is made and ordinary rotated camera
photos stay within the budget.

Tests: unit `decoded_pixels_beyond_the_memory_budget_are_refused_before_decoding`
(a 7000x7000 PNG of ~730 KB, within the dimension bound, whose 147,000,000
decoded bytes exceed the budget: refused for a chosen photo and for
readback) and `a_24_megapixel_rotated_photo_within_the_budget_is_prepared`
(6000x4000 with orientation 6); the orientation colour test still passes
with orientation applied to the square. Integration: the over-budget photo
sends nothing (no presign, no PUT) and the over-budget served avatar is
rejected. Native C06-N13: a staged 7000x7000 photo said the photo is
unchanged and sent nothing. Probe: removing the reservation fails the
integration guard.

# C06 remediation after attempt-04

Verdict attempt-04: architecture 9, behavior 8, authority/security 8,
validation 8, overall 8; C06-A04-RF01.

## C06-A04-RF01 — bounded resampling

`avatar::reencode` no longer calls `DynamicImage::resize_to_fill` (which
scales the whole image to cover the square before cropping, so an 8192x1
input would need a 4,194,304x512 intermediate). It reads the centred square
in place (`imageops::crop_imm`, no copy) and resamples only that square to
512 pixels (`imageops::resize`, Lanczos3). Before resampling it reserves the
side x 512 RGBA f32 intermediate (16 bytes a pixel) and the 512-pixel RGBA
output from what the 128 MiB budget has left after the decoded pixels;
orientation is then applied to the 512-pixel square.

Tests: unit `extreme_landscape_and_portrait_photos_are_prepared_from_their_centre`
(8192x1 and 1x8192 PNGs prepared to 512x512 and read back),
`decoded_pixels_and_their_resampling_together_stay_within_the_budget`
(6400x6400: decoded pixels fit alone, with the resampling intermediate they
do not: refused before resampling); the orientation, 24-megapixel rotated and
over-budget tests still pass. Integration:
`extreme_landscape_and_portrait_photos_are_prepared_from_their_centre_and_uploaded`
(both strips presigned, stored as 512-pixel JPEGs and confirmed) and a 1x8192
served avatar read back. Native C06-N13: a staged 1x8192 portrait was
prepared, stored (image/jpeg) and confirmed with no load warning. Probe:
removing the resampling reservation fails the unit guard. The whole-image
cover resize is not re-introduced as a probe, because that mutation would
really allocate gigabytes in the probe run.
