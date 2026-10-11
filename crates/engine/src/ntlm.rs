//! NTLM (MS-NLMP) pour HTTP : les trois messages de l'échange (négociation, défi, authentification) et la réponse NTLMv2.
//! Pas de signature ni de chiffrement de session : HTTP n'en demande pas, la réponse d'authentification suffit.

use md4::{Digest as _, Md4};

const SIGNATURE: &[u8; 8] = b"NTLMSSP\0";

const UNICODE: u32 = 0x0000_0001;
const REQUEST_TARGET: u32 = 0x0000_0004;
const NTLM: u32 = 0x0000_0200;
const ALWAYS_SIGN: u32 = 0x0000_8000;
const EXTENDED_SESSION_SECURITY: u32 = 0x0008_0000;
const VERSION_128: u32 = 0x2000_0000;
const KEY_EXCH: u32 = 0x4000_0000;
const VERSION_56: u32 = 0x8000_0000;
const SIGN: u32 = 0x0000_0010;
const SEAL: u32 = 0x0000_0020;
const LM_KEY: u32 = 0x0000_0080;

const AV_EOL: u16 = 0;
const AV_TIMESTAMP: u16 = 7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    pub flags: u32,
    pub server_challenge: [u8; 8],
    /// Les paires AV du message de défi, telles que le serveur les a envoyées.
    pub target_info: Vec<u8>,
    /// L'horodatage du serveur (FILETIME) quand les paires AV en portent un.
    pub timestamp: Option<[u8; 8]>,
}

fn hmac_md5(key: &[u8], data: &[u8]) -> [u8; 16] {
    const BLOCK: usize = 64;
    let mut padded = [0u8; BLOCK];
    if key.len() > BLOCK {
        padded[..16].copy_from_slice(&md5::compute(key).0);
    } else {
        padded[..key.len()].copy_from_slice(key);
    }
    let mut inner: Vec<u8> = padded.iter().map(|b| b ^ 0x36).collect();
    inner.extend_from_slice(data);
    let inner_hash = md5::compute(&inner).0;
    let mut outer: Vec<u8> = padded.iter().map(|b| b ^ 0x5c).collect();
    outer.extend_from_slice(&inner_hash);
    md5::compute(&outer).0
}

fn utf16le(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// Le hachage du mot de passe (MD4 de son UTF-16LE).
pub fn nt_hash(password: &str) -> [u8; 16] {
    Md4::digest(utf16le(password)).into()
}

/// `NTOWFv2` : HMAC-MD5 du hachage du mot de passe sur l'utilisateur en majuscules suivi du domaine.
pub fn ntowf_v2(password: &str, user: &str, domain: &str) -> [u8; 16] {
    hmac_md5(&nt_hash(password), &utf16le(&format!("{}{domain}", user.to_uppercase())))
}

fn field(buffer: &mut Vec<u8>, len: usize, offset: usize) {
    let len = u16::try_from(len).unwrap_or(u16::MAX);
    buffer.extend_from_slice(&len.to_le_bytes());
    buffer.extend_from_slice(&len.to_le_bytes());
    buffer.extend_from_slice(&u32::try_from(offset).unwrap_or(u32::MAX).to_le_bytes());
}

/// Le message de négociation (type 1) : aucun domaine ni poste n'est annoncé, le serveur répond avec son défi.
pub fn negotiate() -> Vec<u8> {
    let flags = UNICODE | REQUEST_TARGET | NTLM | ALWAYS_SIGN | EXTENDED_SESSION_SECURITY | VERSION_128 | VERSION_56;
    let mut out = Vec::with_capacity(32);
    out.extend_from_slice(SIGNATURE);
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    field(&mut out, 0, 32);
    field(&mut out, 0, 32);
    out
}

fn u16_at(bytes: &[u8], at: usize) -> Option<usize> {
    Some(usize::from(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?)))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// Lit le message de défi (type 2) ; `None` quand ce n'en est pas un.
pub fn parse_challenge(bytes: &[u8]) -> Option<Challenge> {
    if bytes.get(..8)? != SIGNATURE || u32_at(bytes, 8)? != 2 {
        return None;
    }
    let flags = u32_at(bytes, 20)?;
    let server_challenge: [u8; 8] = bytes.get(24..32)?.try_into().ok()?;
    let (len, offset) = (u16_at(bytes, 40).unwrap_or(0), usize::try_from(u32_at(bytes, 44).unwrap_or(0)).ok()?);
    let target_info = if len == 0 { Vec::new() } else { bytes.get(offset..offset + len)?.to_vec() };
    Some(Challenge { flags, server_challenge, timestamp: timestamp_of(&target_info), target_info })
}

fn timestamp_of(info: &[u8]) -> Option<[u8; 8]> {
    let mut at = 0;
    while at + 4 <= info.len() {
        let id = u16::from_le_bytes([info[at], info[at + 1]]);
        let len = usize::from(u16::from_le_bytes([info[at + 2], info[at + 3]]));
        if id == AV_EOL {
            return None;
        }
        let value = info.get(at + 4..at + 4 + len)?;
        if id == AV_TIMESTAMP && len == 8 {
            return value.try_into().ok();
        }
        at += 4 + len;
    }
    None
}

/// L'instant `unix_millis` en FILETIME (intervalles de 100 ns depuis 1601), sur 8 octets petit-boutiste.
pub fn filetime(unix_millis: u64) -> [u8; 8] {
    (unix_millis.saturating_mul(10_000).saturating_add(116_444_736_000_000_000)).to_le_bytes()
}

/// La réponse NTLMv2 : (réponse LM, réponse NT). Avec un horodatage dans le défi, la réponse LM est vide (24 zéros), comme
/// le demande MS-NLMP.
pub fn responses_v2(
    challenge: &Challenge,
    password: &str,
    user: &str,
    domain: &str,
    client_challenge: [u8; 8],
    timestamp: [u8; 8],
) -> (Vec<u8>, Vec<u8>) {
    let key = ntowf_v2(password, user, domain);
    let mut blob = vec![1, 1, 0, 0, 0, 0, 0, 0];
    blob.extend_from_slice(&timestamp);
    blob.extend_from_slice(&client_challenge);
    blob.extend_from_slice(&[0; 4]);
    blob.extend_from_slice(&challenge.target_info);
    blob.extend_from_slice(&[0; 4]);
    let mut proof_input = challenge.server_challenge.to_vec();
    proof_input.extend_from_slice(&blob);
    let mut nt = hmac_md5(&key, &proof_input).to_vec();
    nt.extend_from_slice(&blob);
    let lm = if challenge.timestamp.is_some() {
        vec![0; 24]
    } else {
        let mut input = challenge.server_challenge.to_vec();
        input.extend_from_slice(&client_challenge);
        let mut lm = hmac_md5(&key, &input).to_vec();
        lm.extend_from_slice(&client_challenge);
        lm
    };
    (lm, nt)
}

/// Le message d'authentification (type 3). Les drapeaux de signature, de chiffrement et d'échange de clé du serveur ne sont
/// pas repris : aucune clé de session n'est envoyée.
pub fn authenticate(
    challenge: &Challenge,
    user: &str,
    password: &str,
    domain: &str,
    workstation: &str,
    client_challenge: [u8; 8],
    now_millis: u64,
) -> Vec<u8> {
    let timestamp = challenge.timestamp.unwrap_or_else(|| filetime(now_millis));
    let (lm, nt) = responses_v2(challenge, password, user, domain, client_challenge, timestamp);
    let unicode = challenge.flags & UNICODE != 0;
    let encode = |text: &str| if unicode { utf16le(text) } else { text.as_bytes().to_vec() };
    let (domain, user, workstation) = (encode(domain), encode(user), encode(workstation));
    let flags = challenge.flags & !(KEY_EXCH | SIGN | SEAL | LM_KEY);

    let header = 64;
    let fields: [&[u8]; 6] = [&lm, &nt, &domain, &user, &workstation, &[]];
    let mut out = Vec::new();
    out.extend_from_slice(SIGNATURE);
    out.extend_from_slice(&3u32.to_le_bytes());
    let mut offset = header;
    for value in fields {
        field(&mut out, value.len(), offset);
        offset += value.len();
    }
    out.extend_from_slice(&flags.to_le_bytes());
    out.resize(header, 0);
    for value in fields {
        out.extend_from_slice(value);
    }
    out
}

/// Découpe `NTLM <base64>` ; `None` pour un autre schéma. Un `NTLM` seul (sans jeton) rend une chaîne vide.
pub fn token_of(header: &str) -> Option<&str> {
    let header = header.trim();
    header.get(..4).filter(|scheme| scheme.eq_ignore_ascii_case("ntlm"))?;
    let tail = &header[4..];
    (tail.is_empty() || tail.starts_with(' ')).then(|| tail.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn ef_aut_03_the_nt_hash_and_the_ntowf_v2_match_the_ms_nlmp_test_vectors() {
        assert_eq!(hex(&nt_hash("Password")), "a4f49c406510bdcab6824ee7c30fd852");
        assert_eq!(hex(&ntowf_v2("Password", "User", "Domain")), "0c868a403bfd7a93a3001ef22ef02e3f");
    }

    #[test]
    fn ef_aut_03_hmac_md5_matches_rfc_2202() {
        assert_eq!(hex(&hmac_md5(&[0x0b; 16], b"Hi There")), "9294727a3638bb1c13f48ef8158bfc9d");
        assert_eq!(hex(&hmac_md5(b"Jefe", b"what do ya want for nothing?")), "750c783e6ab0b503eaa86e310a5db738");
    }

    fn target_info() -> Vec<u8> {
        let mut info = Vec::new();
        for (id, text) in [(2u16, "Domain"), (1, "Server")] {
            let value = utf16le(text);
            info.extend_from_slice(&id.to_le_bytes());
            info.extend_from_slice(&u16::try_from(value.len()).unwrap().to_le_bytes());
            info.extend_from_slice(&value);
        }
        info.extend_from_slice(&[0; 4]);
        info
    }

    #[test]
    fn ef_aut_03_the_ntlm_v2_responses_match_the_ms_nlmp_test_vectors() {
        let challenge = Challenge {
            flags: 0xe2888235,
            server_challenge: [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef],
            target_info: target_info(),
            timestamp: None,
        };

        let (lm, nt) = responses_v2(&challenge, "Password", "User", "Domain", [0xaa; 8], [0; 8]);

        assert_eq!(hex(&lm), "86c35097ac9cec102554764a57cccc19aaaaaaaaaaaaaaaa");
        assert_eq!(hex(&nt[..16]), "68cd0ab851e51c96aabc927bebef6a1c");
    }

    #[test]
    fn ef_aut_03_the_negotiate_message_is_a_type_1_message() {
        let message = negotiate();
        assert_eq!(&message[..8], b"NTLMSSP\0");
        assert_eq!(u32::from_le_bytes(message[8..12].try_into().unwrap()), 1);
        assert_eq!(message.len(), 32);
    }

    fn type2(info: &[u8]) -> Vec<u8> {
        let mut message = Vec::new();
        message.extend_from_slice(b"NTLMSSP\0");
        message.extend_from_slice(&2u32.to_le_bytes());
        message.extend_from_slice(&[0; 8]);
        message.extend_from_slice(&0xe288_8235u32.to_le_bytes());
        message.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        message.extend_from_slice(&[0; 8]);
        message.extend_from_slice(&u16::try_from(info.len()).unwrap().to_le_bytes());
        message.extend_from_slice(&u16::try_from(info.len()).unwrap().to_le_bytes());
        message.extend_from_slice(&56u32.to_le_bytes());
        message.extend_from_slice(&[0; 8]);
        message.extend_from_slice(info);
        message
    }

    #[test]
    fn ef_aut_03_a_challenge_is_read_with_its_server_challenge_target_info_and_timestamp() {
        let mut info = target_info();
        info.truncate(info.len() - 4);
        info.extend_from_slice(&[7, 0, 8, 0, 1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 0, 0]);

        let challenge = parse_challenge(&type2(&info)).unwrap();

        assert_eq!(challenge.server_challenge, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(challenge.flags, 0xe288_8235);
        assert_eq!(challenge.target_info, info);
        assert_eq!(challenge.timestamp, Some([1, 2, 3, 4, 5, 6, 7, 8]));
        assert!(parse_challenge(b"NTLMSSP\0\x01\0\0\0").is_none());
        assert!(parse_challenge(b"nope").is_none());
    }

    #[test]
    fn ef_aut_03_the_authenticate_message_carries_the_user_domain_and_workstation_and_drops_session_flags() {
        let challenge = parse_challenge(&type2(&target_info())).unwrap();

        let message = authenticate(&challenge, "ada", "pw", "CORP", "PC1", [9; 8], 1_000);

        assert_eq!(&message[..8], b"NTLMSSP\0");
        assert_eq!(u32::from_le_bytes(message[8..12].try_into().unwrap()), 3);
        let flags = u32::from_le_bytes(message[60..64].try_into().unwrap());
        assert_eq!(flags & (KEY_EXCH | SIGN | SEAL), 0);
        let text = String::from_utf16_lossy(
            &message[64..]
                .chunks(2)
                .filter(|c| c.len() == 2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect::<Vec<_>>(),
        );
        assert!(text.contains("CORP") && text.contains("ada") && text.contains("PC1"), "{text:?}");
        let (lm_len, nt_len) = (u16_at(&message, 12).unwrap(), u16_at(&message, 20).unwrap());
        assert_eq!(lm_len, 24);
        assert!(nt_len > 16 + 28);
    }

    #[test]
    fn ef_aut_03_a_timestamp_in_the_challenge_empties_the_lm_response() {
        let mut info = target_info();
        info.truncate(info.len() - 4);
        info.extend_from_slice(&[7, 0, 8, 0, 1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 0, 0]);
        let challenge = parse_challenge(&type2(&info)).unwrap();

        let (lm, _) = responses_v2(&challenge, "pw", "ada", "CORP", [9; 8], [1, 2, 3, 4, 5, 6, 7, 8]);

        assert_eq!(lm, vec![0; 24]);
    }

    #[test]
    fn ef_aut_03_the_header_token_is_cut_from_the_ntlm_scheme_only() {
        assert_eq!(token_of("NTLM TlRMTVNTUAAC"), Some("TlRMTVNTUAAC"));
        assert_eq!(token_of("ntlm"), Some(""));
        assert_eq!(token_of("  NTLM  abc  "), Some("abc"));
        assert_eq!(token_of("Negotiate abc"), None);
        assert_eq!(token_of("NTLMX abc"), None);
    }

    #[test]
    fn ef_aut_03_the_filetime_of_the_unix_epoch_is_the_1601_offset() {
        assert_eq!(u64::from_le_bytes(filetime(0)), 116_444_736_000_000_000);
        assert_eq!(u64::from_le_bytes(filetime(1_000)), 116_444_736_000_000_000 + 10_000_000);
    }
}
