//! ISO 639-1 two-letter language codes as a closed enum.
//!
//! Stored as a single SCALE enum-tag byte on-chain — both cheaper and
//! stricter than a `BoundedVec<u8, ConstU32<2>>` while covering every
//! language with a commercial recording catalogue. New codes are not
//! expected: ISO 639-1 has been frozen for over two decades.

use parity_scale_codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;

macro_rules! define_languages {
    ($($variant:ident = $code:literal),* $(,)?) => {
        /// ISO 639-1 alpha-2 language code.
        ///
        /// JSON shape: lowercase string matching the ISO 639-1 code (e.g.
        /// `Language::En` ↔ `"en"`), aligned with [`Language::as_code`].
        #[derive(
            Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
            Clone, Copy, PartialEq, Eq, Debug,
        )]
        #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
        #[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
        // Variant names are the ISO 639-1 codes themselves.
        #[allow(missing_docs)]
        pub enum Language {
            $($variant,)*
        }

        impl Language {
            /// Canonical two-letter code.
            pub const fn as_code(self) -> &'static [u8; 2] {
                match self {
                    $(Self::$variant => $code,)*
                }
            }

            /// Parse a two-byte ISO 639-1 code (case-sensitive lookup —
            /// canonical ISO 639-1 codes are lowercase).
            pub fn from_code(code: &[u8]) -> Option<Self> {
                match code {
                    $($code => Some(Self::$variant),)*
                    _ => None,
                }
            }

            /// Parse a two-byte ISO 639-1 code, accepting any ASCII case
            /// (e.g. `b"EN"`, `b"En"`, `b"en"` all resolve to
            /// [`Language::En`]). Useful when ingesting metadata from
            /// upstream sources that aren't strict about case.
            pub fn from_code_ignore_ascii_case(code: &[u8]) -> Option<Self> {
                if code.len() != 2 {
                    return None;
                }
                let lower: [u8; 2] = [
                    code[0].to_ascii_lowercase(),
                    code[1].to_ascii_lowercase(),
                ];
                Self::from_code(&lower)
            }
        }
    };
}

define_languages! {
    Aa = b"aa", Ab = b"ab", Ae = b"ae", Af = b"af", Ak = b"ak",
    Am = b"am", An = b"an", Ar = b"ar", As = b"as", Av = b"av",
    Ay = b"ay", Az = b"az",
    Ba = b"ba", Be = b"be", Bg = b"bg", Bh = b"bh", Bi = b"bi",
    Bm = b"bm", Bn = b"bn", Bo = b"bo", Br = b"br", Bs = b"bs",
    Ca = b"ca", Ce = b"ce", Ch = b"ch", Co = b"co", Cr = b"cr",
    Cs = b"cs", Cu = b"cu", Cv = b"cv", Cy = b"cy",
    Da = b"da", De = b"de", Dv = b"dv", Dz = b"dz",
    Ee = b"ee", El = b"el", En = b"en", Eo = b"eo", Es = b"es",
    Et = b"et", Eu = b"eu",
    Fa = b"fa", Ff = b"ff", Fi = b"fi", Fj = b"fj", Fo = b"fo",
    Fr = b"fr", Fy = b"fy",
    Ga = b"ga", Gd = b"gd", Gl = b"gl", Gn = b"gn", Gu = b"gu",
    Gv = b"gv",
    Ha = b"ha", He = b"he", Hi = b"hi", Ho = b"ho", Hr = b"hr",
    Ht = b"ht", Hu = b"hu", Hy = b"hy", Hz = b"hz",
    Ia = b"ia", Id = b"id", Ie = b"ie", Ig = b"ig", Ii = b"ii",
    Ik = b"ik", Io = b"io", Is = b"is", It = b"it", Iu = b"iu",
    Ja = b"ja", Jv = b"jv",
    Ka = b"ka", Kg = b"kg", Ki = b"ki", Kj = b"kj", Kk = b"kk",
    Kl = b"kl", Km = b"km", Kn = b"kn", Ko = b"ko", Kr = b"kr",
    Ks = b"ks", Ku = b"ku", Kv = b"kv", Kw = b"kw", Ky = b"ky",
    La = b"la", Lb = b"lb", Lg = b"lg", Li = b"li", Ln = b"ln",
    Lo = b"lo", Lt = b"lt", Lu = b"lu", Lv = b"lv",
    Mg = b"mg", Mh = b"mh", Mi = b"mi", Mk = b"mk", Ml = b"ml",
    Mn = b"mn", Mr = b"mr", Ms = b"ms", Mt = b"mt", My = b"my",
    Na = b"na", Nb = b"nb", Nd = b"nd", Ne = b"ne", Ng = b"ng",
    Nl = b"nl", Nn = b"nn", No = b"no", Nr = b"nr", Nv = b"nv",
    Ny = b"ny",
    Oc = b"oc", Oj = b"oj", Om = b"om", Or = b"or", Os = b"os",
    Pa = b"pa", Pi = b"pi", Pl = b"pl", Ps = b"ps", Pt = b"pt",
    Qu = b"qu",
    Rm = b"rm", Rn = b"rn", Ro = b"ro", Ru = b"ru", Rw = b"rw",
    Sa = b"sa", Sc = b"sc", Sd = b"sd", Se = b"se", Sg = b"sg",
    Si = b"si", Sk = b"sk", Sl = b"sl", Sm = b"sm", Sn = b"sn",
    So = b"so", Sq = b"sq", Sr = b"sr", Ss = b"ss", St = b"st",
    Su = b"su", Sv = b"sv", Sw = b"sw",
    Ta = b"ta", Te = b"te", Tg = b"tg", Th = b"th", Ti = b"ti",
    Tk = b"tk", Tl = b"tl", Tn = b"tn", To = b"to", Tr = b"tr",
    Ts = b"ts", Tt = b"tt", Tw = b"tw", Ty = b"ty",
    Ug = b"ug", Uk = b"uk", Ur = b"ur", Uz = b"uz",
    Ve = b"ve", Vi = b"vi", Vo = b"vo",
    Wa = b"wa", Wo = b"wo",
    Xh = b"xh",
    Yi = b"yi", Yo = b"yo",
    Za = b"za", Zh = b"zh", Zu = b"zu",
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_common_codes() {
        for (raw, expected) in [
            (&b"en"[..], Language::En),
            (b"fr", Language::Fr),
            (b"es", Language::Es),
            (b"de", Language::De),
            (b"ja", Language::Ja),
            (b"zh", Language::Zh),
            (b"ar", Language::Ar),
            (b"ru", Language::Ru),
        ] {
            let parsed = Language::from_code(raw).expect("known code");
            assert_eq!(parsed, expected);
            assert_eq!(parsed.as_code(), raw);
        }
    }

    #[test]
    fn unknown_code_returns_none() {
        assert!(Language::from_code(b"xx").is_none());
        assert!(Language::from_code(b"").is_none());
        assert!(Language::from_code(b"eng").is_none());
        assert!(Language::from_code(b"EN").is_none());
    }

    #[test]
    fn from_code_ignore_ascii_case_normalises_both_orientations() {
        for raw in [&b"en"[..], b"EN", b"En", b"eN"] {
            assert_eq!(
                Language::from_code_ignore_ascii_case(raw),
                Some(Language::En),
                "{:?}",
                raw,
            );
        }
        assert!(Language::from_code_ignore_ascii_case(b"xx").is_none());
        assert!(Language::from_code_ignore_ascii_case(b"eng").is_none());
        assert!(Language::from_code_ignore_ascii_case(b"e").is_none());
    }

    #[test]
    fn encoded_size_is_one_byte() {
        let encoded = Language::En.encode();
        assert_eq!(encoded.len(), 1);
    }

    #[test]
    fn max_encoded_len_is_one() {
        assert_eq!(<Language as MaxEncodedLen>::max_encoded_len(), 1);
    }
}
