use super::*;

fn store() -> Arc<Store> {
    static NEXT_STORE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let directory = format!(
        "{}/authbot_bot_test_{}_{}_{}",
        std::env::temp_dir().display(),
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT_STORE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let _ = std::fs::remove_dir_all(&directory);
    let path = format!("{directory}/authbot.db");
    Arc::new(Store::open(&path).unwrap())
}

#[test]
fn cancel_command_accepts_telegram_addressing_but_no_arguments() {
    assert!(is_cancel_command("/cancel"));
    assert!(is_cancel_command("/cancel@ClaudeApiBot"));
    assert!(is_cancel_command("/cancel@bot_123"));
    assert!(!is_cancel_command("/cancel "));
    assert!(!is_cancel_command("/cancel now"));
    assert!(!is_cancel_command("/cancel@"));
    assert!(!is_cancel_command("/cancel@bad-name"));
    assert!(!is_cancel_command("cancel"));
}

/// Продукт оффера разводит три несовместимые передачи доступа. В частности, Gemini никогда не
/// должен попасть в `claude setup-token`, даже если позже изменится подпись кнопки.
#[test]
fn product_decides_which_handover_the_seller_gets() {
    for codex in [
        "ChatGPT Plus",
        "ChatGPT Pro",
        "chatgpt plus",
        "GPT-5 аккаунт",
    ] {
        assert_eq!(handoff_kind(codex), HandoffKind::Codex);
    }
    for claude in ["Claude Pro", "Claude 5x", "Claude 20x", "claude pro"] {
        assert_eq!(handoff_kind(claude), HandoffKind::Claude);
    }
    for gemini in [
        "Google AI Pro",
        "Google AI Ultra",
        "Code Assist Standard",
        "Code Assist Enterprise",
        "Workspace AI Ultra",
    ] {
        assert_eq!(handoff_kind(gemini), HandoffKind::Gemini);
    }
}

/// Каждая кнопка продукта должна резолвиться в имя, которое потом правильно классифицируется.
/// Иначе новый ярлык в меню тихо уедет в Claude-ветку.
#[test]
fn every_product_button_resolves_and_classifies() {
    for row in product_kb() {
        for (label, data) in row {
            let code = data.strip_prefix("noffer:").expect("product button");
            let name = tier_name(code).expect("every button has a product name");
            assert_eq!(name, label, "button label and product name must match");
            let expected = if label.contains("GLM") {
                HandoffKind::Glm
            } else if label.contains("Kimi") {
                HandoffKind::Kimi
            } else if label.contains("Gemini")
                || label.contains("Google AI")
                || label.contains("Code Assist")
                || label.contains("Workspace AI")
            {
                HandoffKind::Gemini
            } else if label.contains("ChatGPT") {
                HandoffKind::Codex
            } else {
                HandoffKind::Claude
            };
            assert_eq!(handoff_kind(name), expected, "{label} classified wrongly");
        }
    }
}

#[test]
fn product_menus_show_only_the_two_operator_selected_gemini_plans() {
    let persistent_buttons = admin_home_kb().into_iter().flatten().collect::<Vec<_>>();
    let offer_buttons = product_kb()
        .into_iter()
        .flatten()
        .map(|(label, _)| label)
        .collect::<Vec<_>>();
    let visible = [
        ("📦 Google AI Pro", "Google AI Pro"),
        ("📦 Google AI Ultra", "Google AI Ultra"),
    ];
    for (button, product) in visible {
        assert!(
            persistent_buttons.contains(&button),
            "missing persistent button {button}"
        );
        assert!(offer_buttons.iter().any(|label| label == product));
        assert_eq!(admin_quick_tier(button), Some(product));
        assert_eq!(handoff_kind(product), HandoffKind::Gemini);
    }

    let hidden = [
        ("📦 Code Assist Standard", "Code Assist Standard"),
        ("📦 Code Assist Enterprise", "Code Assist Enterprise"),
        ("📦 Workspace AI Ultra", "Workspace AI Ultra"),
    ];
    for (button, product) in hidden {
        assert!(
            !persistent_buttons.contains(&button),
            "retired persistent button {button} is still visible"
        );
        assert!(!offer_buttons.iter().any(|label| label == product));
        // Old reply keyboards and callbacks remain routable during rollout.
        assert_eq!(admin_quick_tier(button), Some(product));
        assert_eq!(handoff_kind(product), HandoffKind::Gemini);
    }
}

#[test]
fn persistent_admin_keyboard_exposes_both_chatgpt_products() {
    let buttons = admin_home_kb().into_iter().flatten().collect::<Vec<_>>();
    for (button, product) in [
        ("📦 ChatGPT Plus", "ChatGPT Plus"),
        ("📦 ChatGPT Pro", "ChatGPT Pro"),
    ] {
        assert!(buttons.contains(&button), "missing ChatGPT button {button}");
        assert_eq!(admin_quick_tier(button), Some(product));
        assert_eq!(handoff_kind(product), HandoffKind::Codex);
    }
}

#[test]
fn batch_product_menu_covers_every_subscription_variant() {
    let labels = batch_product_kb()
        .into_iter()
        .flatten()
        .map(|(label, data)| {
            let code = data
                .strip_prefix("nbatch:")
                .expect("batch product callback");
            (label, tier_name(code).expect("batch product code"))
        })
        .collect::<Vec<_>>();
    assert_eq!(labels.len(), 15);
    for (label, product) in labels {
        assert_eq!(label, product);
        assert!(matches!(
            handoff_kind(product),
            HandoffKind::Claude
                | HandoffKind::Codex
                | HandoffKind::Gemini
                | HandoffKind::Kimi
                | HandoffKind::Glm
        ));
    }
}

#[test]
fn stepping_back_from_an_issued_kimi_device_code_invalidates_it() {
    // The seller already holds a live code; going back must burn it, not leave two valid
    // codes racing to publish into the same deal.
    let back = handoff_step_back(HandoffKind::Kimi, "km_wait", true, true).expect("edge");
    assert_eq!(back.target, "km_ready");
    assert!(back.invalidates_link);
    assert!(
        !back.clears_proxy,
        "the pinned egress must survive a step back"
    );
}

#[test]
fn a_kimi_step_back_without_a_recoverable_egress_degrades_to_the_proxy_step() {
    // Landing on km_ready with an empty hproxy would be a dead end: kimi_ready_handoff
    // rejects it and the seller could never continue.
    let back = handoff_step_back(HandoffKind::Kimi, "km_wait", true, false).expect("edge");
    assert_eq!(back.target, "km_proxy");
    assert!(back.clears_proxy);
    // A seller who may not replace the proxy has no such step in their history at all.
    assert!(handoff_step_back(HandoffKind::Kimi, "km_wait", false, false).is_none());
}

#[test]
fn kimi_ready_steps_back_to_its_own_proxy_step() {
    let back = handoff_step_back(HandoffKind::Kimi, "km_ready", true, true).expect("edge");
    assert_eq!(back.target, "km_proxy");
    assert!(!back.invalidates_link, "no code has been issued yet");
}

#[test]
fn the_kimi_wait_confirmation_warns_that_the_code_dies() {
    let text = handoff_back_confirm_text("km_wait");
    assert!(text.contains("перестанет работать"));
    assert!(text.contains("уже подтвердил"));
}

#[test]
fn the_kimi_ready_button_carries_its_own_callback() {
    let keyboard = kimi_ready_kb(None);
    assert_eq!(keyboard[0][0].1, "kimi:ready");
    // A shared callback would let one provider's button advance another provider's deal.
    assert_ne!(keyboard[0][0].1, gemini_ready_kb(None)[0][0].1);
}

#[test]
fn the_kimi_ready_gate_requires_both_its_step_and_a_stored_proxy() {
    let store = store();
    // Unknown seller, wrong step, or a right step with no stored egress must all leave the
    // button inert: starting an acquisition without the assigned proxy would authorize from
    // a different IP than the one the account was opened on.
    assert!(kimi_ready_handoff(&store, 1).is_none());
    let _ = store.set_want(1, "km_ready");
    assert!(kimi_ready_handoff(&store, 1).is_none());
}

#[test]
fn kimi_products_are_a_distinct_handoff_and_never_fall_through_to_claude() {
    // A new product silently classified as Claude would be handed to the setup-token branch
    // and burn a paid subscription on the wrong flow.
    for product in [
        "Kimi Andante",
        "Kimi Moderato",
        "Kimi Allegretto",
        "Kimi Allegro",
        "Kimi Vivace",
    ] {
        assert_eq!(handoff_kind(product), HandoffKind::Kimi, "{product}");
    }
    assert_eq!(handoff_kind("Moonshot Kimi Code"), HandoffKind::Kimi);
}

#[test]
fn kimi_plan_words_cannot_be_claimed_by_another_provider_rule() {
    // The plan names are generic musical terms, so classification must key on the provider
    // word rather than on the tier word.
    for bare in ["Andante", "Moderato", "Allegretto", "Allegro", "Vivace"] {
        assert_ne!(
            handoff_kind(bare),
            HandoffKind::Kimi,
            "{bare} must not be treated as a KIMI product without the provider name"
        );
    }
}

#[test]
fn kimi_offers_route_to_their_own_wizard_steps_and_texts() {
    assert_eq!(
        handoff_steps_for_product("Kimi Moderato"),
        ("km_proxy", "km_ready")
    );
    // Each provider owns distinct steps: a shared step id would let one provider's callback
    // advance another provider's deal.
    for other in ["Claude Pro", "ChatGPT Plus", "Google AI Pro"] {
        assert_ne!(handoff_steps_for_product(other).0, "km_proxy");
        assert_ne!(handoff_steps_for_product(other).1, "km_ready");
    }
    assert_eq!(seller_offer_guide("Kimi Allegretto"), KIMI_OFFER_GUIDE);
    assert_eq!(account_setup_prompt("km_ready"), KIMI_ACCOUNT_SETUP);
    assert_eq!(proxy_prompt("km_proxy"), KIMI_PROXY_PROMPT);
}

#[test]
fn kimi_seller_texts_demand_the_coding_plan_and_never_ask_for_secrets() {
    // The consumer Kimi chat subscription does not grant API access, so buying it would cost
    // a payout and deliver nothing routable.
    assert!(KIMI_ACCOUNT_SETUP.contains("Kimi Code"));
    assert!(
        KIMI_ACCOUNT_SETUP.contains("чат") || KIMI_ACCOUNT_SETUP.contains("не подходит"),
        "the seller must be told the consumer chat plan grants no API access"
    );
    // The guide must carry the explicit disclaimer, since that is what a seller reads before
    // deciding whether a request for credentials is legitimate.
    assert!(KIMI_OFFER_GUIDE.contains("бот не просит"));
    // No text may instruct the seller to hand over account credentials. The word "пароль"
    // appears legitimately in the proxy field explanation, so the check targets the phrasing
    // that would actually solicit an account secret.
    for text in [KIMI_OFFER_GUIDE, KIMI_ACCOUNT_SETUP, KIMI_PROXY_PROMPT] {
        let lowered = text.to_lowercase();
        for forbidden in [
            "пришли пароль",
            "пароль от аккаунта",
            "пришли код из",
            "пришли cookie",
            "пришли токен",
            "код 2fa",
        ] {
            assert!(
                !lowered.contains(forbidden),
                "seller text must never solicit: {forbidden}"
            );
        }
    }
}

#[test]
fn kimi_appears_in_both_operator_menus() {
    let offer_codes = product_kb()
        .into_iter()
        .flatten()
        .filter_map(|(_, data)| data.strip_prefix("noffer:").map(str::to_string))
        .filter(|code| code.starts_with("kimi_"))
        .count();
    assert_eq!(offer_codes, 5);
    for label in [
        "📦 Kimi Andante",
        "📦 Kimi Moderato",
        "📦 Kimi Allegretto",
        "📦 Kimi Allegro",
        "📦 Kimi Vivace",
    ] {
        assert!(admin_quick_tier(label).is_some(), "{label} missing");
        assert!(
            admin_home_kb().iter().flatten().any(|b| *b == label),
            "{label} missing from the persistent keyboard"
        );
    }
}

/// Regression: a Kimi seller on `km_proxy` sent an HTTP proxy and the bot answered
/// «Доступна только команда /start.» — the step simply had no text arm. The decision is a
/// pure function so the accepted/rejected shapes are pinned without a Telegram mock.
#[test]
fn kimi_proxy_step_accepts_and_reconstructs_seller_input() {
    let store = store();
    store.register_user(111, 111, "kimi-seller").unwrap();
    let offer = store
        .create_offer("Kimi Allegretto", "$20", 999, 111)
        .unwrap();
    let reference = SellerJobRef {
        kind: "offer".into(),
        offer_id: offer,
        batch_id: 0,
        item_no: 0,
        token: "generation".into(),
    };
    assert_eq!(
        select_kimi_proxy_input(&store, &reference, "", 0, "1.2.3.4:8080:user:pass"),
        KimiProxyInput::SellerSupplied("http://user:pass@1.2.3.4:8080".into(), true)
    );
    // Пароль с двоеточием обязан пережить реконструкцию: режем ровно на четыре поля и
    // процент-кодируем userinfo, иначе в CONNECT уедет чужой пароль.
    assert_eq!(
        select_kimi_proxy_input(&store, &reference, "", 0, "1.2.3.4:8080:user:pa:ss"),
        KimiProxyInput::SellerSupplied("http://user:pa%3Ass@1.2.3.4:8080".into(), true)
    );
    assert_eq!(
        select_kimi_proxy_input(&store, &reference, "", 0, "http://user:pass@1.2.3.4:8080"),
        KimiProxyInput::SellerSupplied("http://user:pass@1.2.3.4:8080".into(), true)
    );
    // Авторизация по IP законна, но продавец получает явное предупреждение о ней.
    assert_eq!(
        select_kimi_proxy_input(&store, &reference, "", 0, "1.2.3.4:8080"),
        KimiProxyInput::SellerSupplied("http://1.2.3.4:8080".into(), false)
    );
}

#[test]
fn kimi_proxy_step_rejects_malformed_input_without_leaking_it() {
    let store = store();
    store.register_user(111, 111, "kimi-seller").unwrap();
    let offer = store
        .create_offer("Kimi Allegretto", "$20", 999, 111)
        .unwrap();
    let reference = SellerJobRef {
        kind: "offer".into(),
        offer_id: offer,
        batch_id: 0,
        item_no: 0,
        token: "generation".into(),
    };
    for rejected in ["", "не прокси", "1.2.3.4", "1.2.3.4:abc", "1.2.3.4:0"] {
        assert_eq!(
            select_kimi_proxy_input(&store, &reference, "", 0, rejected),
            KimiProxyInput::Invalid,
            "{rejected:?}"
        );
    }
    // Отпечаток отвергнутого ввода — единственное, что попадает в журнал: ни логина, ни пароля.
    let fingerprint = proxy_input_fingerprint("1.2.3.4:8080:secret-user:secret-pass");
    assert!(!fingerprint.contains("secret-user"), "{fingerprint}");
    assert!(!fingerprint.contains("secret-pass"), "{fingerprint}");
}

#[test]
fn kimi_proxy_step_never_replaces_a_pinned_proxy() {
    let store = store();
    store.register_user(111, 111, "kimi-seller").unwrap();
    let offer = store
        .create_offer("Kimi Allegretto", "$20", 999, 111)
        .unwrap();
    let reference = SellerJobRef {
        kind: "offer".into(),
        offer_id: offer,
        batch_id: 0,
        item_no: 0,
        token: "generation".into(),
    };
    // Оплаченный лиз закреплён: валидное сообщение продавца не может его заменить.
    store.mark_offer_proxy_issued(offer).unwrap();
    assert_eq!(
        select_kimi_proxy_input(
            &store,
            &reference,
            "http://user:pass@5.6.7.8:8080",
            0,
            "1.2.3.4:8080:user:pass"
        ),
        KimiProxyInput::Fixed("http://user:pass@5.6.7.8:8080".into(), 0)
    );
    // Закреплённый egress потерян: принимать ввод продавца нельзя, остаёмся fail-closed.
    assert_eq!(
        select_kimi_proxy_input(&store, &reference, "", 0, "1.2.3.4:8080:user:pass"),
        KimiProxyInput::Invalid
    );
}

#[test]
fn kimi_seller_proxy_is_canonicalised_before_it_is_pinned() {
    // parse_proxy_input проверяет только форму сообщения; мусор в URL-форме проходит форму,
    // но не должен закрепляться за аккаунтом — иначе продавец застрял бы на km_ready с
    // egress, на котором device-flow не может начаться.
    let shaped = parse_proxy_input("http://foo bar:8080");
    assert!(!shaped.url.is_empty());
    assert!(kimi_credential::normalize_proxy_url(&shaped.url).is_err());
    // Канонический вывод обеих форм ввода одинаково валиден для credential-контракта.
    for raw in [
        "1.2.3.4:8080:user:pa:ss",
        "http://user:pa%3Ass@1.2.3.4:8080",
    ] {
        let parsed = parse_proxy_input(raw);
        assert!(
            kimi_credential::normalize_proxy_url(&parsed.url).is_ok(),
            "{raw}"
        );
    }
}

#[test]
fn kimi_proxy_step_has_clean_manual_and_retry_prompts() {
    assert!(!KIMI_PROXY_PROMPT.contains("не получилось"));
    assert_eq!(
        manual_proxy_prompt("km_proxy"),
        format!("{MANUAL_PROXY_WARNING}{KIMI_PROXY_PROMPT}")
    );
    assert!(KIMI_STEP_PROXY_RETRY.contains("ip:port:user:pass"));
    // Подсказка не может содержать сам присланный прокси: только форма, никаких секретов.
    assert!(!KIMI_STEP_PROXY_RETRY.contains("<code>1"));
}

#[test]
fn glm_products_are_a_distinct_handoff_and_never_fall_through_to_claude() {
    // A new product silently classified as Claude would be handed to the setup-token branch
    // and burn a paid subscription on the wrong flow.
    for product in [
        "GLM Coding Plan Lite",
        "GLM Coding Plan Pro",
        "GLM Coding Plan Max",
    ] {
        assert_eq!(handoff_kind(product), HandoffKind::Glm, "{product}");
    }
    assert_eq!(handoff_kind("Zhipu GLM Coding Plan"), HandoffKind::Glm);
    assert_eq!(handoff_kind("Z.ai Coding Plan Pro"), HandoffKind::Glm);
    assert_eq!(handoff_kind("bigmodel.cn coding plan"), HandoffKind::Glm);
}

#[test]
fn glm_tier_words_cannot_be_claimed_by_another_provider_rule() {
    // The tier names are generic — Claude sells a Max too — so classification must key on
    // the provider/platform words, never on the bare tier word.
    for bare in ["Lite", "Pro", "Max"] {
        assert_ne!(
            handoff_kind(bare),
            HandoffKind::Glm,
            "{bare} must not be treated as a GLM product without the provider name"
        );
    }
    // The full product name must not be claimed by the Claude fallback either, even though
    // Claude has its own Max.
    assert_eq!(handoff_kind("GLM Coding Plan Max"), HandoffKind::Glm);
}

#[test]
fn glm_offers_route_to_their_own_wizard_steps_and_texts() {
    assert_eq!(
        handoff_steps_for_product("GLM Coding Plan Pro"),
        ("glm_proxy", "glm_ready")
    );
    // Each provider owns distinct steps: a shared step id would let one provider's callback
    // advance another provider's deal.
    for other in [
        "Claude Pro",
        "ChatGPT Plus",
        "Google AI Pro",
        "Kimi Moderato",
    ] {
        assert_ne!(handoff_steps_for_product(other).0, "glm_proxy");
        assert_ne!(handoff_steps_for_product(other).1, "glm_ready");
    }
    assert_eq!(seller_offer_guide("GLM Coding Plan Lite"), GLM_OFFER_GUIDE);
    assert_eq!(account_setup_prompt("glm_ready"), GLM_ACCOUNT_SETUP);
    assert_eq!(proxy_prompt("glm_proxy"), GLM_PROXY_PROMPT);
}

#[test]
fn glm_appears_in_both_operator_menus() {
    let offer_codes = product_kb()
        .into_iter()
        .flatten()
        .filter_map(|(_, data)| data.strip_prefix("noffer:").map(str::to_string))
        .filter(|code| code.starts_with("glm_"))
        .count();
    assert_eq!(offer_codes, 3);
    for label in [
        "📦 GLM Coding Plan Lite",
        "📦 GLM Coding Plan Pro",
        "📦 GLM Coding Plan Max",
    ] {
        assert!(admin_quick_tier(label).is_some(), "{label} missing");
        assert!(
            admin_home_kb().iter().flatten().any(|b| *b == label),
            "{label} missing from the persistent keyboard"
        );
    }
}

#[test]
fn glm_seller_texts_demand_the_individual_plan_and_never_ask_for_secrets() {
    // Team and pay-as-you-go shapes do not match the corroborated credits ladder, so buying
    // the wrong product would cost a payout and deliver nothing routable.
    assert!(GLM_ACCOUNT_SETUP.contains("Individual Coding Plan"));
    assert!(
        GLM_ACCOUNT_SETUP.contains("Team-версия"),
        "the seller must be told the Team version does not fit"
    );
    assert!(GLM_ACCOUNT_SETUP.contains("Plan Overview"));
    assert!(GLM_OFFER_GUIDE.contains("бот не просит"));
    // The API key is the only credential artifact: the key prompt asks for it alone, and no
    // text may instruct the seller to hand over account credentials. The word "пароль"
    // appears legitimately in the proxy field explanation, so the check targets the
    // phrasing that would actually solicit an account secret.
    assert!(GLM_KEY_PROMPT.contains("API-ключ"));
    for text in [
        GLM_OFFER_GUIDE,
        GLM_ACCOUNT_SETUP,
        GLM_PROXY_PROMPT,
        GLM_KEY_PROMPT,
    ] {
        let lowered = text.to_lowercase();
        for forbidden in [
            "пришли пароль",
            "пароль от аккаунта",
            "пришли код из",
            "пришли cookie",
            "пришли токен",
            "код 2fa",
        ] {
            assert!(
                !lowered.contains(forbidden),
                "seller text must never solicit: {forbidden}"
            );
        }
    }
}

#[test]
fn glm_proxy_step_accepts_and_reconstructs_seller_input() {
    let store = store();
    store.register_user(111, 111, "glm-seller").unwrap();
    let offer = store
        .create_offer("GLM Coding Plan Pro", "$20", 999, 111)
        .unwrap();
    let reference = SellerJobRef {
        kind: "offer".into(),
        offer_id: offer,
        batch_id: 0,
        item_no: 0,
        token: "generation".into(),
    };
    assert_eq!(
        select_glm_proxy_input(&store, &reference, "", 0, "1.2.3.4:8080:user:pass"),
        GlmProxyInput::SellerSupplied("http://user:pass@1.2.3.4:8080".into(), true)
    );
    // Пароль с двоеточием обязан пережить реконструкцию: режем ровно на четыре поля и
    // процент-кодируем userinfo, иначе в CONNECT уедет чужой пароль.
    assert_eq!(
        select_glm_proxy_input(&store, &reference, "", 0, "1.2.3.4:8080:user:pa:ss"),
        GlmProxyInput::SellerSupplied("http://user:pa%3Ass@1.2.3.4:8080".into(), true)
    );
    assert_eq!(
        select_glm_proxy_input(&store, &reference, "", 0, "http://user:pass@1.2.3.4:8080"),
        GlmProxyInput::SellerSupplied("http://user:pass@1.2.3.4:8080".into(), true)
    );
    // Авторизация по IP законна, но продавец получает явное предупреждение о ней.
    assert_eq!(
        select_glm_proxy_input(&store, &reference, "", 0, "1.2.3.4:8080"),
        GlmProxyInput::SellerSupplied("http://1.2.3.4:8080".into(), false)
    );
}

#[test]
fn glm_proxy_step_rejects_malformed_input_without_leaking_it() {
    let store = store();
    store.register_user(111, 111, "glm-seller").unwrap();
    let offer = store
        .create_offer("GLM Coding Plan Pro", "$20", 999, 111)
        .unwrap();
    let reference = SellerJobRef {
        kind: "offer".into(),
        offer_id: offer,
        batch_id: 0,
        item_no: 0,
        token: "generation".into(),
    };
    for rejected in ["", "не прокси", "1.2.3.4", "1.2.3.4:abc", "1.2.3.4:0"] {
        assert_eq!(
            select_glm_proxy_input(&store, &reference, "", 0, rejected),
            GlmProxyInput::Invalid,
            "{rejected:?}"
        );
    }
    // Отпечаток отвергнутого ввода — единственное, что попадает в журнал: ни логина, ни пароля.
    let fingerprint = proxy_input_fingerprint("1.2.3.4:8080:secret-user:secret-pass");
    assert!(!fingerprint.contains("secret-user"), "{fingerprint}");
    assert!(!fingerprint.contains("secret-pass"), "{fingerprint}");
}

#[test]
fn glm_proxy_step_never_replaces_a_pinned_proxy() {
    let store = store();
    store.register_user(111, 111, "glm-seller").unwrap();
    let offer = store
        .create_offer("GLM Coding Plan Pro", "$20", 999, 111)
        .unwrap();
    let reference = SellerJobRef {
        kind: "offer".into(),
        offer_id: offer,
        batch_id: 0,
        item_no: 0,
        token: "generation".into(),
    };
    // Оплаченный лиз закреплён: валидное сообщение продавца не может его заменить.
    store.mark_offer_proxy_issued(offer).unwrap();
    assert_eq!(
        select_glm_proxy_input(
            &store,
            &reference,
            "http://user:pass@5.6.7.8:8080",
            0,
            "1.2.3.4:8080:user:pass"
        ),
        GlmProxyInput::Fixed("http://user:pass@5.6.7.8:8080".into(), 0)
    );
    // Закреплённый egress потерян: принимать ввод продавца нельзя, остаёмся fail-closed.
    assert_eq!(
        select_glm_proxy_input(&store, &reference, "", 0, "1.2.3.4:8080:user:pass"),
        GlmProxyInput::Invalid
    );
}

#[test]
fn glm_seller_proxy_is_canonicalised_before_it_is_pinned() {
    // parse_proxy_input проверяет только форму сообщения; мусор в URL-форме проходит форму,
    // но не должен закрепляться за аккаунтом — иначе продавец застрял бы на glm_ready с
    // egress, на котором проверка ключа не может начаться.
    let shaped = parse_proxy_input("http://foo bar:8080");
    assert!(!shaped.url.is_empty());
    assert!(glm_credential::normalize_proxy_url(&shaped.url).is_err());
    // Канонический вывод обеих форм ввода одинаково валиден для credential-контракта.
    for raw in [
        "1.2.3.4:8080:user:pa:ss",
        "http://user:pa%3Ass@1.2.3.4:8080",
    ] {
        let parsed = parse_proxy_input(raw);
        assert!(
            glm_credential::normalize_proxy_url(&parsed.url).is_ok(),
            "{raw}"
        );
    }
}

#[test]
fn glm_proxy_step_has_clean_manual_and_retry_prompts() {
    assert!(!GLM_PROXY_PROMPT.contains("не получилось"));
    assert_eq!(
        manual_proxy_prompt("glm_proxy"),
        format!("{MANUAL_PROXY_WARNING}{GLM_PROXY_PROMPT}")
    );
    assert!(GLM_STEP_PROXY_RETRY.contains("ip:port:user:pass"));
    // Подсказка не может содержать сам присланный прокси: только форма, никаких секретов.
    assert!(!GLM_STEP_PROXY_RETRY.contains("<code>1"));
}

#[test]
fn the_glm_ready_button_carries_its_own_callback() {
    let keyboard = glm_ready_kb(None);
    assert_eq!(keyboard[0][0].1, "glm:ready");
    // A shared callback would let one provider's button advance another provider's deal.
    assert_ne!(keyboard[0][0].1, kimi_ready_kb(None)[0][0].1);
    assert_ne!(keyboard[0][0].1, gemini_ready_kb(None)[0][0].1);
    // The platform selection rides the same card, with its own callbacks.
    assert_eq!(keyboard[1][0].1, "glm:region:int");
    assert_eq!(keyboard[1][1].1, "glm:region:cn");
}

#[test]
fn the_glm_ready_gate_requires_both_its_step_and_a_stored_proxy() {
    let store = store();
    // Unknown seller, wrong step, or a right step with no stored egress must all leave the
    // button inert: validating a key without the assigned proxy would authorize from a
    // different IP than the one the account was opened on.
    assert!(glm_ready_handoff(&store, 1).is_none());
    let _ = store.set_want(1, "glm_ready");
    assert!(glm_ready_handoff(&store, 1).is_none());
}

#[test]
fn stepping_back_from_glm_wait_requires_confirmation_and_invalidates() {
    // The seller already armed the key intake; going back must cancel it, not leave an
    // in-flight validation racing to publish into a rewound deal.
    let back = handoff_step_back(HandoffKind::Glm, "glm_wait", true, true).expect("edge");
    assert_eq!(back.target, "glm_ready");
    assert!(back.invalidates_link);
    assert!(
        !back.clears_proxy,
        "the pinned egress must survive a step back"
    );
    // Одноразовое ожидание молча не гасим: сначала явное подтверждение.
    let row = handoff_back_row(&back, back_step_wire("glm_wait").unwrap());
    assert_eq!(row[0].1, "hoback:glm_wait:ask");
}

#[test]
fn a_glm_step_back_without_a_recoverable_egress_degrades_to_the_proxy_step() {
    // Landing on glm_ready with an empty hproxy would be a dead end: glm_ready_handoff
    // rejects it and the seller could never continue.
    let back = handoff_step_back(HandoffKind::Glm, "glm_wait", true, false).expect("edge");
    assert_eq!(back.target, "glm_proxy");
    assert!(back.clears_proxy);
    // A seller who may not replace the proxy has no such step in their history at all.
    assert!(handoff_step_back(HandoffKind::Glm, "glm_wait", false, false).is_none());
}

#[test]
fn glm_ready_steps_back_to_its_own_proxy_step() {
    let back = handoff_step_back(HandoffKind::Glm, "glm_ready", true, true).expect("edge");
    assert_eq!(back.target, "glm_proxy");
    assert!(
        !back.invalidates_link,
        "the key intake has not been armed yet"
    );
}

#[test]
fn the_glm_wait_confirmation_warns_that_the_key_intake_dies() {
    let text = handoff_back_confirm_text("glm_wait");
    assert!(text.contains("сброшено"));
    assert!(text.contains("уже прислал"));
}

#[test]
fn glm_region_selection_persists_into_the_credential() {
    let store = store();
    store.register_user(111, 111, "glm-seller").unwrap();
    // Default — международная площадка.
    let region = store.get_user(111).unwrap().unwrap().hregion;
    assert_eq!(
        glm_base_url(&region),
        glm_credential::GLM_BASE_URL_INTERNATIONAL
    );
    store.set_hregion(111, "cn").unwrap();
    let region = store.get_user(111).unwrap().unwrap().hregion;
    assert_eq!(glm_base_url(&region), glm_credential::GLM_BASE_URL_CHINA);
    // Выбор доживает до credential_from: ключ одной площадки не работает на другой.
    let credential = glm_key::credential_from(
        "zai-key-1",
        glm_credential::GlmPlan::Max,
        glm_base_url("cn"),
        "",
    )
    .unwrap();
    assert_eq!(credential.base_url, glm_credential::GLM_BASE_URL_CHINA);
    let credential = glm_key::credential_from(
        "zai-key-1",
        glm_credential::GlmPlan::Max,
        glm_base_url("int"),
        "",
    )
    .unwrap();
    assert_eq!(
        credential.base_url,
        glm_credential::GLM_BASE_URL_INTERNATIONAL
    );
    // Сброс возвращает международный default.
    store.set_hregion(111, "").unwrap();
    let region = store.get_user(111).unwrap().unwrap().hregion;
    assert_eq!(
        glm_base_url(&region),
        glm_credential::GLM_BASE_URL_INTERNATIONAL
    );
}

#[test]
fn glm_declared_plan_comes_from_the_offer_product() {
    assert_eq!(
        glm_declared_plan("GLM Coding Plan Lite"),
        Some(glm_credential::GlmPlan::Lite)
    );
    assert_eq!(
        glm_declared_plan("GLM Coding Plan Pro"),
        Some(glm_credential::GlmPlan::Pro)
    );
    assert_eq!(
        glm_declared_plan("GLM Coding Plan Max"),
        Some(glm_credential::GlmPlan::Max)
    );
    // Голое слово тарифа не становится GLM-планом: классификация обязана подтвердить
    // провайдера, а без тарифа сделка fail-closed.
    assert_eq!(glm_declared_plan("Claude Pro"), None);
    assert_eq!(glm_declared_plan("Max"), None);
    assert_eq!(glm_declared_plan("GLM Coding Plan"), None);
}

#[test]
fn malformed_key_text_never_reaches_the_provider() {
    let oversized = "x".repeat(513);
    for rejected in [
        "",
        "  ",
        "key with spaces",
        "multi\nline",
        oversized.as_str(),
    ] {
        assert_eq!(glm_key_text(rejected), None, "{rejected:?}");
    }
    assert_eq!(glm_key_text("  zai-key-abc123  "), Some("zai-key-abc123"));
    // Журнал видит только длину ключа — никогда сам ключ.
    let fingerprint = glm_key_fingerprint("zai-secret-key-9f8c7b");
    assert!(!fingerprint.contains("zai-secret"), "{fingerprint}");
    assert!(!fingerprint.contains("9f8c7b"), "{fingerprint}");
}

#[test]
fn invalid_key_guidance_is_typed_static_and_never_carries_the_key() {
    // Каждый класс отказа платной проверки — своя подсказка продавцу.
    let reasons = [
        glm_key::InvalidKeyReason::Auth,
        glm_key::InvalidKeyReason::OutOfPlanBalance,
        glm_key::InvalidKeyReason::PlanExpired,
        glm_key::InvalidKeyReason::ModelOutOfPlan,
        glm_key::InvalidKeyReason::FairUse,
        glm_key::InvalidKeyReason::WrongKeyKind,
    ];
    let texts: std::collections::HashSet<&'static str> = reasons
        .iter()
        .map(|reason| glm_invalid_key_guidance(*reason))
        .collect();
    assert_eq!(
        texts.len(),
        reasons.len(),
        "each invalid class needs its own guidance"
    );
    // Исчерпанная квота — НЕ невалидный ключ: отдельная подсказка «пришлите позже/другой».
    assert!(GLM_QUOTA_EXHAUSTED.contains("действителен"));
    assert!(GLM_QUOTA_EXHAUSTED.contains("квота"));
    assert_ne!(GLM_QUOTA_EXHAUSTED, GLM_KEY_REJECTED);
    // Подсказки статические и не содержат ни ключа продавца, ни формулировок-просьб секретов.
    let submitted_key = "zai-secret-key-9f8c7b";
    for text in reasons
        .iter()
        .map(|reason| glm_invalid_key_guidance(*reason))
        .into_iter()
        .chain([
            GLM_KEY_REJECTED,
            GLM_PLAN_MISMATCH,
            GLM_PLAN_SHAPE,
            GLM_QUOTA_EXHAUSTED,
            GLM_VALIDATION_TRANSPORT,
            GLM_KEY_MALFORMED,
        ])
    {
        assert!(
            !text.contains(submitted_key),
            "guidance must be static, never an echo of the submitted key"
        );
        let lowered = text.to_lowercase();
        for forbidden in ["пришли пароль", "пароль от аккаунта", "пришли cookie"]
        {
            assert!(!lowered.contains(forbidden), "{forbidden}");
        }
    }
}

#[test]
fn proxy_source_is_visible_and_changes_seller_instructions() {
    let store = store();
    let seller_proxy = store
        .create_offer_with_proxy("Google AI Pro", "$20", 1, 2, PROXY_SOURCE_SELLER, "")
        .unwrap();
    let buyer_proxy = store
        .create_offer_with_proxy(
            "Google AI Pro",
            "$20",
            1,
            2,
            PROXY_SOURCE_BUYER,
            "http://user:pass@1.2.3.4:8080",
        )
        .unwrap();
    let seller_text = offer_text(&store.get_offer(seller_proxy).unwrap().unwrap());
    let buyer_text = offer_text(&store.get_offer(buyer_proxy).unwrap().unwrap());
    assert!(seller_text.contains("от продавца"));
    assert!(seller_text.contains("прислать свой HTTP-прокси"));
    assert!(buyer_text.contains("от покупателя"));
    assert!(buyer_text.contains("Дождаться выплаты"));
    assert!(
        !buyer_text.contains("user:pass@1.2.3.4"),
        "proxy must not leak into offer text"
    );
}

#[test]
fn batch_source_keyboard_has_exactly_the_two_requested_flows() {
    let keyboard = proxy_source_kb("batchproxy");
    assert_eq!(keyboard.len(), 2);
    assert_eq!(keyboard[0][0].1, "batchproxy:buyer");
    assert_eq!(keyboard[1][0].1, "batchproxy:seller");
    assert_eq!(proxy_source_label(PROXY_SOURCE_BUYER), "от покупателя");
    assert_eq!(proxy_source_label(PROXY_SOURCE_SELLER), "от продавца");
}

#[test]
fn batch_quantity_accepts_only_plain_integers() {
    assert_eq!(parse_quantity("10"), Some(10));
    assert_eq!(parse_quantity(" 10 "), Some(10));
    assert_eq!(parse_quantity("10.0"), None);
    assert_eq!(parse_quantity("10 подписок"), None);
    assert_eq!(parse_quantity(""), None);
}

#[test]
fn jobs_card_exposes_progress_and_role_safe_batch_controls() {
    let mut single = SellerJob {
        seller_chat: 42,
        reference: SellerJobRef {
            kind: "offer".into(),
            offer_id: 9,
            batch_id: 0,
            item_no: 0,
            token: "0123456789abcdef0123456789abcdef".into(),
        },
        product: "ChatGPT Plus".into(),
        phase: "accepted".into(),
        total: 1,
    };
    assert!(single_job_kb(&single, false).is_none());
    let single_admin = single_job_kb(&single, true).unwrap();
    assert_eq!(single_admin[0][0].1, "odel:9:ask");
    assert!(
        format!("odel:{}:{}", i64::MAX, single.reference.token).len() <= 64,
        "confirmation callback must fit Telegram's limit"
    );
    single.phase = "paying".into();
    assert!(single_job_kb(&single, true).is_none());
    single.phase = "processing".into();
    assert!(single_job_kb(&single, true).is_some());

    let mut overview = BatchOverview {
        batch: PurchaseBatch {
            id: 7,
            product: "Google AI Pro".into(),
            unit_price: "$20".into(),
            quantity: 5,
            total_price: "$100".into(),
            created_by: 1,
            seller_chat: 42,
            proxy_source: PROXY_SOURCE_BUYER.into(),
            status: "processing".into(),
            payment_tx: "0xtx".into(),
            current_item: 3,
        },
        completed: 2,
        remaining: 3,
    };
    assert_eq!(progress_bar(2, 5), "▓▓▓▓░░░░░░");
    let seller = batch_jobs_kb(&overview, false).unwrap();
    assert_eq!(seller.len(), 1);
    assert_eq!(seller[0][0].1, "batchpause:7:ask");
    let admin = batch_jobs_kb(&overview, true).unwrap();
    assert!(admin
        .iter()
        .flatten()
        .any(|button| button.1 == "batchpause:7:ask"));
    assert!(admin
        .iter()
        .flatten()
        .any(|button| button.1 == "batchrewind:7:2:ask"));
    assert!(admin
        .iter()
        .flatten()
        .any(|button| button.1 == "batchdelete:7:ask"));

    overview.batch.status = "paused".into();
    let seller = batch_jobs_kb(&overview, false).unwrap();
    assert_eq!(seller[0][0].1, "batchresume:7");
    assert!(!seller
        .iter()
        .flatten()
        .any(|button| button.1.contains("delete")));
}

#[test]
fn every_subscription_offer_prepares_a_first_time_seller() {
    let store = store();
    let claude_id = store.create_offer("Claude Pro", "$20", 1, 2).unwrap();
    let chatgpt_id = store.create_offer("ChatGPT Plus", "$20", 1, 2).unwrap();
    let gemini_id = store.create_offer("Google AI Pro", "$20", 1, 2).unwrap();
    let claude = offer_text(&store.get_offer(claude_id).unwrap().unwrap());
    let chatgpt = offer_text(&store.get_offer(chatgpt_id).unwrap().unwrap());
    let gemini = offer_text(&store.get_offer(gemini_id).unwrap().unwrap());

    for guide in [&claude, &chatgpt, &gemini] {
        assert!(guide.contains("антидетект-браузере"));
        assert!(guide.contains("персонального HTTP-прокси"));
        assert!(guide.contains("Пароль, cookie, банковские данные"));
        assert!(guide.chars().count() < 3_500, "Telegram offer is too long");
    }
    assert!(claude.contains("Не регистрируй и не открывай аккаунт"));
    assert!(chatgpt.contains("Не регистрируй и не открывай аккаунт"));
    assert!(gemini.contains("Не регистрируй и не открывай Google-аккаунт"));
    assert!(claude.contains("весь адрес из адресной строки"));
    assert!(chatgpt.contains("одноразовый код"));
    assert!(gemini.contains("зарегистрировать новый Google-аккаунт"));
    assert!(gemini.contains("Аккаунт готов"));

    for setup in [
        CLAUDE_ACCOUNT_SETUP,
        CODEX_ACCOUNT_SETUP,
        GEMINI_ACCOUNT_SETUP,
    ] {
        assert!(setup.contains("новый чистый профиль"));
        assert!(setup.contains("https://accounts.google.com"));
        assert!(setup.contains("IP — первое поле"));
        assert!(setup.contains("Дополнительный VPN не включай"));
        assert!(setup.chars().count() < 3_500, "Telegram prompt is too long");
    }
    assert!(CLAUDE_ACCOUNT_SETUP.contains("Continue with Google"));
    assert!(CODEX_ACCOUNT_SETUP.contains("Continue with Google"));
    assert!(CLAUDE_ACCOUNT_SETUP.contains("точный email"));
    assert!(CODEX_ACCOUNT_SETUP.contains("точный email"));
    assert!(CLAUDE_ACCOUNT_SETUP.contains("https://claude.ai"));
    assert!(CODEX_ACCOUNT_SETUP.contains("https://chatgpt.com"));
    assert!(GEMINI_ACCOUNT_SETUP.contains("Google AI Pro или Ultra"));
    assert!(GEMINI_ACCOUNT_SETUP.contains("https://one.google.com"));
    assert!(GEMINI_ACCOUNT_SETUP.contains("Аккаунт готов — продолжить"));
    assert!(CLAUDE_PROXY_PROMPT.contains("аккаунта Claude"));
    assert!(CODEX_PROXY_PROMPT.contains("аккаунта ChatGPT"));
    assert!(GEMINI_PROXY_PROMPT.contains("аккаунта Gemini"));
    for prompt in [CLAUDE_PROXY_PROMPT, CODEX_PROXY_PROMPT, GEMINI_PROXY_PROMPT] {
        assert!(prompt.contains("регистрация и дальнейшая авторизация"));
        assert!(prompt.contains("ip:port:user:pass"));
    }
}

/// Предупреждение о неудавшейся автовыдаче отделено от самого промпта: осознанный шаг назад
/// продавца сопровождать им нельзя — там ничего не ломалось.
#[test]
fn proxy_prompt_keeps_the_manual_warning_separate() {
    for step in ["ho_proxy", "cx_proxy", "gm_gproxy"] {
        let clean = proxy_prompt(step);
        assert!(!clean.contains("не получилось"), "{step}");
        assert_eq!(
            manual_proxy_prompt(step),
            format!("{MANUAL_PROXY_WARNING}{clean}"),
            "{step}"
        );
    }
    assert_eq!(proxy_prompt("gm_gproxy"), GEMINI_PROXY_PROMPT);
    assert_eq!(proxy_prompt("cx_proxy"), CODEX_PROXY_PROMPT);
    // Неизвестный шаг падает на Claude — так же, как в прямом направлении.
    assert_eq!(proxy_prompt("ho_proxy"), CLAUDE_PROXY_PROMPT);
    assert_eq!(proxy_prompt("что-то ещё"), CLAUDE_PROXY_PROMPT);
}

#[test]
fn seller_copy_contains_actions_not_internal_implementation_notes() {
    assert_eq!(PRODUCT_PICK, "📦 <b>Создание оффера</b>\nВыбери продукт:");
    let seller_copy = [
        PRODUCT_PICK,
        CLAUDE_OFFER_GUIDE,
        CODEX_OFFER_GUIDE,
        GEMINI_OFFER_GUIDE,
        CLAUDE_ACCOUNT_SETUP,
        CODEX_ACCOUNT_SETUP,
        GEMINI_ACCOUNT_SETUP,
        MANUAL_PROXY_WARNING,
        CLAUDE_PROXY_PROMPT,
        CODEX_PROXY_PROMPT,
        GEMINI_PROXY_PROMPT,
        accepted_next_step("Google AI Pro", PROXY_SOURCE_LEGACY),
    ];
    for copy in seller_copy {
        for internal_term in [
            "OAuth-клиент",
            "Cloud API",
            "Client ID",
            "Client secret",
            "managed project",
            "consumer project",
            "roster",
        ] {
            assert!(
                !copy.contains(internal_term),
                "seller copy contains internal term {internal_term}: {copy}"
            );
        }
    }
}

#[test]
fn gemini_ready_button_requires_the_right_state_and_a_stored_proxy() {
    let store = store();
    let chat = 42;
    store.register_user(chat, chat, "gemini-seller").unwrap();
    store
        .set_hproxy(chat, "http://user:pass@1.2.3.4:8080")
        .unwrap();
    store.set_hproxy_order(chat, 17).unwrap();

    store.set_want(chat, "gm_gproxy").unwrap();
    assert!(gemini_ready_handoff(&store, chat).is_none());
    store.set_want(chat, "gm_ready").unwrap();
    assert_eq!(
        gemini_ready_handoff(&store, chat),
        Some(("http://user:pass@1.2.3.4:8080".into(), 17))
    );
    store.set_hproxy(chat, "").unwrap();
    assert!(gemini_ready_handoff(&store, chat).is_none());

    let keyboard = gemini_ready_kb(None);
    assert_eq!(keyboard[0][0].0, "✅ Аккаунт готов — продолжить");
    assert_eq!(keyboard[0][0].1, "gemini:ready");
}

#[test]
fn handoff_steps_follow_the_offer_product() {
    let store = store();
    let claude = store.create_offer("Claude 20x", "$100", 1, 2).unwrap();
    let chatgpt = store.create_offer("ChatGPT Pro", "$200", 1, 2).unwrap();
    let gemini = store.create_offer("Google AI Ultra", "$300", 1, 2).unwrap();
    assert_eq!(handoff_steps(&store, claude), ("ho_proxy", "ho_email"));
    assert_eq!(handoff_steps(&store, chatgpt), ("cx_proxy", "cx_email"));
    assert_eq!(handoff_steps(&store, gemini), ("gm_gproxy", "gm_ready"));
    assert_eq!(handoff_steps(&store, 9_999), ("ho_proxy", "ho_email"));
}

#[test]
fn handoff_completion_requires_exact_source_item_and_product_kind() {
    let job = SellerJob {
        seller_chat: 42,
        reference: SellerJobRef {
            kind: "batch".into(),
            offer_id: 0,
            batch_id: 7,
            item_no: 2,
            token: "generation-a".into(),
        },
        product: "Google AI Pro".into(),
        phase: "processing".into(),
        total: 5,
    };
    assert!(seller_job_matches_handoff(
        &job,
        &job.reference,
        HandoffKind::Gemini
    ));
    assert!(!seller_job_matches_handoff(
        &job,
        &SellerJobRef {
            kind: "offer".into(),
            offer_id: 7,
            batch_id: 0,
            item_no: 0,
            token: "generation-a".into(),
        },
        HandoffKind::Gemini
    ));
    assert!(!seller_job_matches_handoff(
        &job,
        &job.reference,
        HandoffKind::Codex
    ));
    let mut stale_generation = job.reference.clone();
    stale_generation.token = "generation-before-rewind".into();
    assert!(!seller_job_matches_handoff(
        &job,
        &stale_generation,
        HandoffKind::Gemini
    ));
}

/// Шаги трёх веток не должны пересекаться: одно и то же состояние в обеих отправило бы
/// продавца в чужой обработчик после перезапуска бота.
#[test]
fn the_three_handovers_never_share_a_step_name() {
    let claude = ["ho_proxy", "ho_email", "ho_code"];
    let codex = ["cx_proxy", "cx_email", "cx_wait"];
    let gemini = ["gm_gid", "gm_gsecret", "gm_gproxy", "gm_ready", "gm_wait"];
    for step in claude {
        assert!(!codex.contains(&step) && !gemini.contains(&step));
    }
    for step in codex {
        assert!(!gemini.contains(&step));
    }
}

/// Обратная таблица обязана быть полной и однозначной: кнопка «назад» и сама мутация читают
/// ровно её, поэтому любая лишняя или потерянная строка немедленно разводит UI и состояние.
#[test]
fn handoff_back_maps_every_seller_step_to_exactly_one_predecessor() {
    use HandoffKind::{Claude, Codex, Gemini};
    let reversible = [
        (Claude, "ho_email", "ho_proxy", true, false),
        (Claude, "ho_code", "ho_email", false, true),
        (Codex, "cx_email", "cx_proxy", true, false),
        (Codex, "cx_wait", "cx_email", false, true),
        (Gemini, "gm_ready", "gm_gproxy", true, false),
        (Gemini, "gm_wait", "gm_ready", false, true),
    ];
    for (kind, want, target, clears_proxy, invalidates_link) in reversible {
        assert_eq!(
            handoff_step_back(kind, want, true, true),
            Some(HandoffStepBack {
                target,
                clears_proxy,
                invalidates_link,
            }),
            "{kind:?}/{want}"
        );
    }

    // Первые шаги веток, legacy-состояния Gemini, регистрация и пустое состояние назад не ходят.
    for (kind, want) in [
        (Claude, "ho_proxy"),
        (Codex, "cx_proxy"),
        (Gemini, "gm_gproxy"),
        (Gemini, "gm_gid"),
        (Gemini, "gm_gsecret"),
        (Claude, "reg_address"),
        (Claude, ""),
        (Claude, "нет такого шага"),
    ] {
        assert_eq!(
            handoff_step_back(kind, want, true, true),
            None,
            "{kind:?}/{want} не должен иметь предшественника"
        );
    }

    // Шаг чужой ветки — не предшественник, а рассинхрон: продукт работы решает всё.
    for (kind, want) in [
        (Claude, "gm_wait"),
        (Claude, "cx_email"),
        (Codex, "ho_code"),
        (Codex, "gm_ready"),
        (Gemini, "ho_email"),
        (Gemini, "cx_wait"),
    ] {
        assert_eq!(
            handoff_step_back(kind, want, true, true),
            None,
            "{kind:?}/{want}"
        );
    }
}

/// Закреплённый прокси покупателя и живой IPRoyal-лиз не имеют шага «ввод прокси» в истории
/// продавца. Шаги после выдачи ссылки при этом обязаны оставаться обратимыми.
#[test]
fn handoff_back_refuses_to_unpin_a_pinned_proxy() {
    use HandoffKind::{Claude, Codex, Gemini};
    for (kind, want) in [
        (Claude, "ho_email"),
        (Codex, "cx_email"),
        (Gemini, "gm_ready"),
    ] {
        assert_eq!(handoff_step_back(kind, want, false, true), None, "{kind:?}");
    }
    for (kind, want, target) in [
        (Claude, "ho_code", "ho_email"),
        (Codex, "cx_wait", "cx_email"),
        (Gemini, "gm_wait", "gm_ready"),
    ] {
        assert_eq!(
            handoff_step_back(kind, want, false, true).map(|step| step.target),
            Some(target),
            "{kind:?}"
        );
    }
}

/// `gm_wait` — единственное двухисходное ребро. Без восстановленного egress возврат на
/// `gm_ready` дал бы состояние с пустым `hproxy`, которое `gemini_ready_handoff` отвергает,
/// поэтому деградируем до ввода прокси — и только когда продавцу вообще можно его менять.
#[test]
fn handoff_back_from_gm_wait_degrades_to_the_proxy_step_only_when_it_is_replaceable() {
    assert_eq!(
        handoff_step_back(HandoffKind::Gemini, "gm_wait", true, false),
        Some(HandoffStepBack {
            target: "gm_gproxy",
            clears_proxy: true,
            invalidates_link: true,
        })
    );
    // Закреплённый прокси плюс потерянная сессия: возвращать некуда, тупик не создаём.
    assert_eq!(
        handoff_step_back(HandoffKind::Gemini, "gm_wait", false, false),
        None
    );
}

/// Callback data приходит от пользователя: резолвер обязан видеть только whitelist шагов.
#[test]
fn handoff_back_callback_data_covers_exactly_the_reversible_steps() {
    for step in [
        "ho_email",
        "ho_code",
        "cx_email",
        "cx_wait",
        "gm_ready",
        "gm_wait",
        "km_ready",
        "km_wait",
        "glm_ready",
        "glm_wait",
    ] {
        assert_eq!(back_step_wire(step), Some(step));
    }
    for rejected in [
        "ho_proxy",
        "cx_proxy",
        "gm_gproxy",
        "km_proxy",
        "glm_proxy",
        "reg_address",
        "",
        "gm_wait:go",
        "../../etc/passwd",
    ] {
        assert_eq!(back_step_wire(rejected), None, "{rejected:?}");
    }
}

/// Старый legacy-оффер мог получить IPRoyal lease до provider-wide propagation order id.
/// Durable `proxy_issued` обязан по-прежнему закреплять такой egress.
#[test]
fn legacy_claude_offer_with_an_issued_lease_is_not_proxy_replaceable() {
    let store = store();
    store.register_user(111, 111, "claude-seller").unwrap();
    let offer = store
        .create_offer("Claude Max20x", "$20", 999, 111)
        .unwrap();
    let reference = SellerJobRef {
        kind: "offer".into(),
        offer_id: offer,
        batch_id: 0,
        item_no: 0,
        token: "generation".into(),
    };
    // До выдачи прокси legacy-оффер заменяем: продавец сам присылает egress.
    assert!(job_accepts_seller_proxy(&store, &reference, 0));
    store.mark_offer_proxy_issued(offer).unwrap();
    // Legacy-строка без propagated order всё равно закреплена durable-флагом.
    assert!(!job_accepts_seller_proxy(&store, &reference, 0));

    // Gemini-путь не меняется: там закреплённость видна и по номеру заказа.
    let gemini = store
        .create_offer("Google AI Pro", "$20", 999, 111)
        .unwrap();
    let gemini_reference = SellerJobRef {
        kind: "offer".into(),
        offer_id: gemini,
        batch_id: 0,
        item_no: 0,
        token: "generation".into(),
    };
    assert!(job_accepts_seller_proxy(&store, &gemini_reference, 0));
    assert!(!job_accepts_seller_proxy(&store, &gemini_reference, 4242));
}

/// Кнопка обязана нести исходный шаг и правильное действие: молча гасить одноразовую ссылку
/// нельзя, а на безопасном ребре лишний экран подтверждения только мешает.
#[test]
fn handoff_back_button_action_matches_the_edge() {
    use HandoffKind::{Claude, Codex, Gemini};
    for (kind, want) in [
        (Claude, "ho_email"),
        (Codex, "cx_email"),
        (Gemini, "gm_ready"),
    ] {
        let step = handoff_step_back(kind, want, true, true).expect("ребро есть");
        let row = handoff_back_row(&step, back_step_wire(want).unwrap());
        assert_eq!(row.len(), 1);
        assert_eq!(row[0].0, "↩️ Изменить прокси");
        assert_eq!(row[0].1, format!("hoback:{want}:go"));
    }
    for (kind, want, label) in [
        (Claude, "ho_code", "↩️ Назад: другой email"),
        (Codex, "cx_wait", "↩️ Назад: другой email"),
        (Gemini, "gm_wait", "↩️ Назад: новая ссылка"),
    ] {
        let step = handoff_step_back(kind, want, true, true).expect("ребро есть");
        let row = handoff_back_row(&step, back_step_wire(want).unwrap());
        assert_eq!(row[0].0, label);
        // Одноразовую ссылку гасим только после явного подтверждения.
        assert_eq!(row[0].1, format!("hoback:{want}:ask"));
    }
}

/// «Назад» и «повторить» — разные рёбра: второе означает «тот же прокси, новое поколение,
/// остаться на шаге» и обязано продолжать работать как раньше.
#[test]
fn typed_back_word_is_distinct_from_the_retry_word() {
    for accepted in ["назад", "Назад", "  НАЗАД  ", "back", "/back"] {
        assert!(is_handoff_back(accepted), "{accepted:?}");
    }
    for rejected in ["повторить", "повтори", "retry", "", "назад пожалуйста"]
    {
        assert!(!is_handoff_back(rejected), "{rejected:?}");
    }
    assert!(!is_gemini_proxy_retry("назад"));
    assert!(is_gemini_proxy_retry("повторить"));
}

/// Прокси продавца приходит в разных формах; в реестр и в `proxy.url` должен уходить URL.
#[test]
fn seller_proxy_forms_normalise_to_a_url() {
    assert_eq!(
        proxy_url("1.2.3.4:8080:user:pass"),
        "http://user:pass@1.2.3.4:8080"
    );
    assert_eq!(proxy_url("1.2.3.4:8080"), "http://1.2.3.4:8080");
    assert_eq!(
        proxy_url("http://user:pass@1.2.3.4:8080"),
        "http://user:pass@1.2.3.4:8080"
    );
    assert_eq!(proxy_url("  "), "");
    assert_eq!(proxy_url("не прокси"), "");
}

#[test]
fn lifecycle_allocation_comes_only_from_a_literal_proxy_host() {
    assert_eq!(
        literal_proxy_ip("http://user:pass@192.0.2.7:8080").unwrap(),
        Some("192.0.2.7".parse().unwrap())
    );
    assert_eq!(
        literal_proxy_ip("http://user:pass@[2001:db8::7]:8080").unwrap(),
        Some("2001:db8::7".parse().unwrap())
    );
    assert_eq!(
        literal_proxy_ip("http://user:pass@managed.example:8080").unwrap(),
        None
    );
}

/// Пароль продавца — произвольная строка. Любая потеря здесь уходит в CONNECT как ЧУЖОЙ
/// пароль и возвращается неотличимым от мёртвого прокси отказом, поэтому реконструкция URL
/// обязана переживать `:`, `%`, `@`, `/`, `?` и `#` без изменения исходных байтов.
#[test]
fn seller_proxy_password_survives_reserved_characters() {
    for password in [
        "pa:ss",
        "pa%41ss",
        "pa@ss",
        "pa/ss",
        "pa?ss",
        "pa#ss",
        "p%s:s/w@rd#1",
    ] {
        let url = proxy_url(&format!("1.2.3.4:8080:user:{password}"));
        let canonical = gemini_credential::normalize_proxy_url(&url)
            .unwrap_or_else(|error| panic!("{password:?} не канонизируется: {error}"));
        let parsed = reqwest::Url::parse(&canonical).expect("canonical proxy URL");
        let decoded = percent_decode(parsed.password().unwrap_or_default());
        assert_eq!(decoded, password, "пароль искажён для {password:?}");
        assert_eq!(percent_decode(parsed.username()), "user");
        assert_eq!(parsed.host_str(), Some("1.2.3.4"));
        assert_eq!(parsed.port(), Some(8080));
    }
}

/// Хелпер теста: `decodeURIComponent`-эквивалент, которым helper восстанавливает userinfo.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or_default();
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                decoded.push(byte);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(decoded).expect("decoded userinfo is UTF-8")
}

/// `ip:port` — валидный прокси с авторизацией по IP, но продавцовская ветка обязана отличать
/// его от полного `ip:port:user:pass`, иначе обрезанная вставка молча уходит без учётных
/// данных и возвращается как CONNECT 407 уже внутри OAuth.
#[test]
fn parsed_proxy_reports_whether_credentials_were_recognised() {
    assert_eq!(
        parse_proxy_input("1.2.3.4:8080:user:pass"),
        ProxyInput {
            url: "http://user:pass@1.2.3.4:8080".into(),
            credentials: true,
        }
    );
    assert_eq!(
        parse_proxy_input("1.2.3.4:8080"),
        ProxyInput {
            url: "http://1.2.3.4:8080".into(),
            credentials: false,
        }
    );
    assert_eq!(
        parse_proxy_input("http://1.2.3.4:8080"),
        ProxyInput {
            url: "http://1.2.3.4:8080".into(),
            credentials: false,
        }
    );
    assert_eq!(
        parse_proxy_input("http://user:pass@1.2.3.4:8080"),
        ProxyInput {
            url: "http://user:pass@1.2.3.4:8080".into(),
            credentials: true,
        }
    );
    for rejected in ["1.2.3.4", "1.2.3.4:port", "1.2.3.4:0", ":8080", "  "] {
        assert_eq!(
            parse_proxy_input(rejected),
            ProxyInput::invalid(),
            "{rejected:?} должен быть отвергнут"
        );
    }
}

/// Отпечаток нужен для разбора инцидента и потому обязан оставаться без секретов.
#[test]
fn rejected_proxy_fingerprint_never_carries_credentials() {
    let fingerprint = proxy_input_fingerprint("1.2.3.4:port:login7:secret42");
    assert_eq!(
        fingerprint,
        "shape=host:port:user:pass host_ok=true port_ok=false user_len=6 pass_len=8"
    );
    assert!(!fingerprint.contains("login7") && !fingerprint.contains("secret42"));
    assert_eq!(proxy_input_fingerprint("   "), "shape=empty");
    assert_eq!(
        proxy_input_fingerprint("1.2.3.4:8080"),
        "shape=host:port host_ok=true port_ok=true credentials=no"
    );
    assert!(proxy_input_fingerprint("http://user:pass@1.2.3.4:8080").starts_with("shape=url"));
    assert!(!proxy_input_fingerprint("http://user:pass@1.2.3.4:8080").contains("pass"));
}

#[test]
fn gemini_retry_consumes_a_new_seller_proxy_but_keeps_a_buyer_proxy_fixed() {
    let seller_store = store();
    seller_store
        .register_user(111, 111, "seller-proxy")
        .unwrap();
    let seller_batch = seller_store
        .create_batch(
            "Google AI Pro",
            "$20",
            2,
            "$40",
            999,
            111,
            PROXY_SOURCE_SELLER,
            &[],
        )
        .unwrap();
    assert!(seller_store.accept_batch(seller_batch, 111).unwrap());
    assert!(seller_store.claim_batch_payment(seller_batch).unwrap());
    assert!(seller_store
        .mark_batch_paid(seller_batch, "0xseller")
        .unwrap());
    assert!(seller_store.start_batch_item(seller_batch, 1).unwrap());
    let seller_job = seller_store.active_seller_job(111).unwrap().unwrap();
    assert!(seller_store
        .set_handoff_state_for_seller_job(
            111,
            &seller_job.reference,
            "gm_gproxy",
            "http://old:proxy@1.1.1.1:8000",
            0,
        )
        .unwrap());
    assert_eq!(
        select_gemini_proxy_retry(
            &seller_store,
            &seller_job.reference,
            "http://old:proxy@1.1.1.1:8000",
            0,
            "2.2.2.2:9000:new:proxy",
        ),
        GeminiProxyRetry::SellerSupplied("http://new:proxy@2.2.2.2:9000".into(), true)
    );
    assert_eq!(
        select_gemini_proxy_retry(
            &seller_store,
            &seller_job.reference,
            "http://old:proxy@1.1.1.1:8000",
            0,
            "повторить",
        ),
        GeminiProxyRetry::Retained("http://old:proxy@1.1.1.1:8000".into(), 0)
    );

    let buyer_store = store();
    buyer_store.register_user(222, 222, "buyer-proxy").unwrap();
    let assigned = "http://fixed:proxy@3.3.3.3:7000".to_string();
    let buyer_batch = buyer_store
        .create_batch(
            "Google AI Pro",
            "$20",
            2,
            "$40",
            999,
            222,
            PROXY_SOURCE_BUYER,
            &[assigned.clone(), "http://next:proxy@4.4.4.4:7000".into()],
        )
        .unwrap();
    assert!(buyer_store.accept_batch(buyer_batch, 222).unwrap());
    assert!(buyer_store.claim_batch_payment(buyer_batch).unwrap());
    assert!(buyer_store.mark_batch_paid(buyer_batch, "0xbuyer").unwrap());
    assert!(buyer_store.start_batch_item(buyer_batch, 1).unwrap());
    let buyer_job = buyer_store.active_seller_job(222).unwrap().unwrap();
    assert_eq!(
        select_gemini_proxy_retry(
            &buyer_store,
            &buyer_job.reference,
            &assigned,
            0,
            "5.5.5.5:9000:ignored:proxy",
        ),
        GeminiProxyRetry::Fixed(assigned, 0)
    );
}

use super::extract_code_state;
#[test]
fn parse_callback_url_and_codestate() {
    let url = "https://platform.claude.com/oauth/code/callback?code=rmkUNDCtEG8zswTyaDn44qFTMN6qLWLOQxGi91XhKEsZhrBp&state=47eEhvUtKx6vcoYLVGCcmkCMCVR7mPDBQF3XBZbGTnk";
    assert_eq!(extract_code_state(url).as_deref(),
        Some("rmkUNDCtEG8zswTyaDn44qFTMN6qLWLOQxGi91XhKEsZhrBp#47eEhvUtKx6vcoYLVGCcmkCMCVR7mPDBQF3XBZbGTnk"));
    assert_eq!(extract_code_state(" abc#xyz ").as_deref(), Some("abc#xyz")); // уже code#state
    assert_eq!(extract_code_state("justcode"), None); // мусор
                                                      // authorize-URL (code=true) не должен ловиться как код
    assert_eq!(
        extract_code_state("https://claude.com/cai/oauth/authorize?code=true&state=zzz"),
        None
    );
}
