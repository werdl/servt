pub(crate) static STATUS_CODES: &[(&str, u16)] = &[
    ("CONTINUE", 100),
    ("SWITCHING PROTOCOLS", 101),
    ("PROCESSING", 102),
    ("EARLY HINTS", 103),
    ("OK", 200),
    ("CREATED", 201),
    ("ACCEPTED", 202),
    ("NON AUTHORITATIVE INFORMATION", 203),
    ("NO CONTENT", 204),
    ("RESET CONTENT", 205),
    ("PARTIAL CONTENT", 206),
    ("MULTI STATUS", 207),
    ("ALREADY REPORTED", 208),
    ("IM USED", 226),
    ("MULTIPLE CHOICES", 300),
    ("MOVED PERMANENTLY", 301),
    ("FOUND", 302),
    ("SEE OTHER", 303),
    ("NOT MODIFIED", 304),
    ("USE PROXY", 305),
    ("TEMPORARY REDIRECT", 307),
    ("PERMANENT REDIRECT", 308),
    ("BAD REQUEST", 400),
    ("UNAUTHORIZED", 401),
    ("PAYMENT REQUIRED", 402),
    ("FORBIDDEN", 403),
    ("NOT FOUND", 404),
    ("METHOD NOT ALLOWED", 405),
    ("NOT ACCEPTABLE", 406),
    ("PROXY AUTHENTICATION REQUIRED", 407),
    ("REQUEST TIMEOUT", 408),
    ("CONFLICT", 409),
    ("GONE", 410),
    ("LENGTH REQUIRED", 411),
    ("PRECONDITION FAILED", 412),
    ("PAYLOAD TOO LARGE", 413),
    ("URI TOO LONG", 414),
    ("UNSUPPORTED MEDIA TYPE", 415),
    ("RANGE NOT SATISFIABLE", 416),
    ("EXPECTATION FAILED", 417),
    ("IM A TEAPOT", 418),
    ("MISDIRECTED REQUEST", 421),
    ("UNPROCESSABLE ENTITY", 422),
    ("LOCKED", 423),
    ("FAILED DEPENDENCY", 424),
    ("TOO EARLY", 425),
    ("UPGRADE REQUIRED", 426),
    ("PRECONDITION REQUIRED", 428),
    ("TOO MANY REQUESTS", 429),
    ("REQUEST HEADER FIELDS TOO LARGE", 431),
    ("UNAVAILABLE FOR LEGAL REASONS", 451),
    ("INTERNAL SERVER ERROR", 500),
    ("NOT IMPLEMENTED", 501),
    ("BAD GATEWAY", 502),
    ("SERVICE UNAVAILABLE", 503),
    ("GATEWAY TIMEOUT", 504),
    ("HTTP VERSION NOT SUPPORTED", 505),
    ("VARIANT ALSO NEGOTIATES", 506),
    ("INSUFFICIENT STORAGE", 507),
    ("LOOP DETECTED", 508),
    ("NOT EXTENDED", 510),
    ("NETWORK AUTHENTICATION REQUIRED", 511),
];

pub(crate) trait CodeLookup {
    fn lookup(&self, code: u16) -> Option<&'static str>;
    fn lookup_code(&self, code: &'static str) -> Option<u16>;

    fn is_valid(&self, code: u16) -> bool {
        self.lookup(code).is_some()
    }
}

impl CodeLookup for &[(&'static str, u16)] {
    fn lookup(&self, code: u16) -> Option<&'static str> {
        self.iter()
            .find_map(|(name, c)| if *c == code { Some(*name) } else { None })
    }

    fn lookup_code(&self, code: &'static str) -> Option<u16> {
        self.iter()
            .find_map(|(name, c)| if *name == code { Some(*c) } else { None })
    }
}
