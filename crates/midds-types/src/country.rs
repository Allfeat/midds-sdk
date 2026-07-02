//! ISO 3166-1 alpha-2 two-letter country codes as a closed enum.
//!
//! Stored as a single SCALE enum-tag byte on-chain — both cheaper and
//! stricter than a `BoundedVec<u8, ConstU32<2>>` while covering every
//! territory a `Release` can be issued in. New codes are rare: ISO 3166-1
//! changes only when a country is created or dissolved, and the runtime
//! absorbs it as a normal additive payload-version bump.
//!
//! Canonical ISO 3166-1 alpha-2 codes are **uppercase** (unlike the
//! lowercase ISO 639-1 language codes), so the JSON shape is the uppercase
//! string (`Country::Fr` ↔ `"FR"`) and [`Country::as_code`] returns the
//! uppercase bytes.

use parity_scale_codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;

macro_rules! define_countries {
    ($($variant:ident = $code:literal),* $(,)?) => {
        /// ISO 3166-1 alpha-2 country code.
        ///
        /// JSON shape: uppercase string matching the ISO 3166-1 alpha-2 code
        /// (e.g. `Country::Fr` ↔ `"FR"`), aligned with [`Country::as_code`].
        #[derive(
            Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen,
            Clone, Copy, PartialEq, Eq, Debug,
        )]
        #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
        #[cfg_attr(feature = "serde", serde(rename_all = "UPPERCASE"))]
        // Variant names are the ISO 3166-1 alpha-2 codes themselves.
        #[allow(missing_docs)]
        pub enum Country {
            $($variant,)*
        }

        impl Country {
            /// Canonical two-letter uppercase code.
            pub const fn as_code(self) -> &'static [u8; 2] {
                match self {
                    $(Self::$variant => $code,)*
                }
            }

            /// Parse a two-byte ISO 3166-1 alpha-2 code (case-sensitive
            /// lookup — canonical codes are uppercase).
            pub fn from_code(code: &[u8]) -> Option<Self> {
                match code {
                    $($code => Some(Self::$variant),)*
                    _ => None,
                }
            }

            /// Parse a two-byte ISO 3166-1 alpha-2 code, accepting any ASCII
            /// case (e.g. `b"fr"`, `b"Fr"`, `b"FR"` all resolve to
            /// [`Country::Fr`]). Useful when ingesting metadata from upstream
            /// sources that aren't strict about case.
            pub fn from_code_ignore_ascii_case(code: &[u8]) -> Option<Self> {
                if code.len() != 2 {
                    return None;
                }
                let upper: [u8; 2] = [
                    code[0].to_ascii_uppercase(),
                    code[1].to_ascii_uppercase(),
                ];
                Self::from_code(&upper)
            }
        }
    };
}

define_countries! {
    Ad = b"AD", Ae = b"AE", Af = b"AF", Ag = b"AG", Ai = b"AI",
    Al = b"AL", Am = b"AM", Ao = b"AO", Aq = b"AQ", Ar = b"AR",
    As = b"AS", At = b"AT", Au = b"AU", Aw = b"AW", Ax = b"AX",
    Az = b"AZ",
    Ba = b"BA", Bb = b"BB", Bd = b"BD", Be = b"BE", Bf = b"BF",
    Bg = b"BG", Bh = b"BH", Bi = b"BI", Bj = b"BJ", Bl = b"BL",
    Bm = b"BM", Bn = b"BN", Bo = b"BO", Bq = b"BQ", Br = b"BR",
    Bs = b"BS", Bt = b"BT", Bv = b"BV", Bw = b"BW", By = b"BY",
    Bz = b"BZ",
    Ca = b"CA", Cc = b"CC", Cd = b"CD", Cf = b"CF", Cg = b"CG",
    Ch = b"CH", Ci = b"CI", Ck = b"CK", Cl = b"CL", Cm = b"CM",
    Cn = b"CN", Co = b"CO", Cr = b"CR", Cu = b"CU", Cv = b"CV",
    Cw = b"CW", Cx = b"CX", Cy = b"CY", Cz = b"CZ",
    De = b"DE", Dj = b"DJ", Dk = b"DK", Dm = b"DM", Do = b"DO",
    Dz = b"DZ",
    Ec = b"EC", Ee = b"EE", Eg = b"EG", Eh = b"EH", Er = b"ER",
    Es = b"ES", Et = b"ET",
    Fi = b"FI", Fj = b"FJ", Fk = b"FK", Fm = b"FM", Fo = b"FO",
    Fr = b"FR",
    Ga = b"GA", Gb = b"GB", Gd = b"GD", Ge = b"GE", Gf = b"GF",
    Gg = b"GG", Gh = b"GH", Gi = b"GI", Gl = b"GL", Gm = b"GM",
    Gn = b"GN", Gp = b"GP", Gq = b"GQ", Gr = b"GR", Gs = b"GS",
    Gt = b"GT", Gu = b"GU", Gw = b"GW", Gy = b"GY",
    Hk = b"HK", Hm = b"HM", Hn = b"HN", Hr = b"HR", Ht = b"HT",
    Hu = b"HU",
    Id = b"ID", Ie = b"IE", Il = b"IL", Im = b"IM", In = b"IN",
    Io = b"IO", Iq = b"IQ", Ir = b"IR", Is = b"IS", It = b"IT",
    Je = b"JE", Jm = b"JM", Jo = b"JO", Jp = b"JP",
    Ke = b"KE", Kg = b"KG", Kh = b"KH", Ki = b"KI", Km = b"KM",
    Kn = b"KN", Kp = b"KP", Kr = b"KR", Kw = b"KW", Ky = b"KY",
    Kz = b"KZ",
    La = b"LA", Lb = b"LB", Lc = b"LC", Li = b"LI", Lk = b"LK",
    Lr = b"LR", Ls = b"LS", Lt = b"LT", Lu = b"LU", Lv = b"LV",
    Ly = b"LY",
    Ma = b"MA", Mc = b"MC", Md = b"MD", Me = b"ME", Mf = b"MF",
    Mg = b"MG", Mh = b"MH", Mk = b"MK", Ml = b"ML", Mm = b"MM",
    Mn = b"MN", Mo = b"MO", Mp = b"MP", Mq = b"MQ", Mr = b"MR",
    Ms = b"MS", Mt = b"MT", Mu = b"MU", Mv = b"MV", Mw = b"MW",
    Mx = b"MX", My = b"MY", Mz = b"MZ",
    Na = b"NA", Nc = b"NC", Ne = b"NE", Nf = b"NF", Ng = b"NG",
    Ni = b"NI", Nl = b"NL", No = b"NO", Np = b"NP", Nr = b"NR",
    Nu = b"NU", Nz = b"NZ",
    Om = b"OM",
    Pa = b"PA", Pe = b"PE", Pf = b"PF", Pg = b"PG", Ph = b"PH",
    Pk = b"PK", Pl = b"PL", Pm = b"PM", Pn = b"PN", Pr = b"PR",
    Ps = b"PS", Pt = b"PT", Pw = b"PW", Py = b"PY",
    Qa = b"QA",
    Re = b"RE", Ro = b"RO", Rs = b"RS", Ru = b"RU", Rw = b"RW",
    Sa = b"SA", Sb = b"SB", Sc = b"SC", Sd = b"SD", Se = b"SE",
    Sg = b"SG", Sh = b"SH", Si = b"SI", Sj = b"SJ", Sk = b"SK",
    Sl = b"SL", Sm = b"SM", Sn = b"SN", So = b"SO", Sr = b"SR",
    Ss = b"SS", St = b"ST", Sv = b"SV", Sx = b"SX", Sy = b"SY",
    Sz = b"SZ",
    Tc = b"TC", Td = b"TD", Tf = b"TF", Tg = b"TG", Th = b"TH",
    Tj = b"TJ", Tk = b"TK", Tl = b"TL", Tm = b"TM", Tn = b"TN",
    To = b"TO", Tr = b"TR", Tt = b"TT", Tv = b"TV", Tw = b"TW",
    Tz = b"TZ",
    Ua = b"UA", Ug = b"UG", Um = b"UM", Us = b"US", Uy = b"UY",
    Uz = b"UZ",
    Va = b"VA", Vc = b"VC", Ve = b"VE", Vg = b"VG", Vi = b"VI",
    Vn = b"VN", Vu = b"VU",
    Wf = b"WF", Ws = b"WS",
    Ye = b"YE", Yt = b"YT",
    Za = b"ZA", Zm = b"ZM", Zw = b"ZW",
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_common_codes() {
        for (raw, expected) in [
            (&b"FR"[..], Country::Fr),
            (b"US", Country::Us),
            (b"GB", Country::Gb),
            (b"DE", Country::De),
            (b"JP", Country::Jp),
            (b"BR", Country::Br),
            (b"ZA", Country::Za),
            (b"AU", Country::Au),
        ] {
            let parsed = Country::from_code(raw).expect("known code");
            assert_eq!(parsed, expected);
            assert_eq!(parsed.as_code(), raw);
        }
    }

    #[test]
    fn unknown_code_returns_none() {
        assert!(Country::from_code(b"XX").is_none());
        assert!(Country::from_code(b"").is_none());
        assert!(Country::from_code(b"FRA").is_none());
        assert!(Country::from_code(b"fr").is_none());
    }

    #[test]
    fn from_code_ignore_ascii_case_normalises_both_orientations() {
        for raw in [&b"FR"[..], b"fr", b"Fr", b"fR"] {
            assert_eq!(
                Country::from_code_ignore_ascii_case(raw),
                Some(Country::Fr),
                "{:?}",
                raw,
            );
        }
        assert!(Country::from_code_ignore_ascii_case(b"XX").is_none());
        assert!(Country::from_code_ignore_ascii_case(b"FRA").is_none());
        assert!(Country::from_code_ignore_ascii_case(b"F").is_none());
    }

    #[test]
    fn encoded_size_is_one_byte() {
        let encoded = Country::Fr.encode();
        assert_eq!(encoded.len(), 1);
    }

    #[test]
    fn max_encoded_len_is_one() {
        assert_eq!(<Country as MaxEncodedLen>::max_encoded_len(), 1);
    }
}
