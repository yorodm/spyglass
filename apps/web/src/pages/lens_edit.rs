use gloo::timers::callback::Timeout;
use ui_components::{
    btn::{Btn, BtnSize},
    icons,
    results::Paginator,
};
use wasm_bindgen::{
    prelude::{wasm_bindgen, Closure},
    JsValue,
};
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;
use yew::html::Scope;
use yew::prelude::*;
use yew_router::scope_ext::RouterScopeExt;

use crate::{
    client::{
        ApiClient, ApiError, GetLensSourceResponse, Lens, LensAddDocType, LensAddDocument,
        LensDocType, LensSource,
    },
    AuthStatus,
};

const QUERY_DEBOUNCE_MS: u32 = 1_000;

#[wasm_bindgen(module = "/public/gapi.js")]
extern "C" {
    #[wasm_bindgen]
    pub fn init_gapi(client_id: &str, api_key: &str);

    #[wasm_bindgen(catch)]
    pub async fn create_picker(cb: &Closure<dyn Fn(JsValue, JsValue)>) -> Result<(), JsValue>;
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = "clearTimeout")]
    fn clear_timeout(handle: JsValue);
}

#[derive(Clone)]
pub struct LensSourcePaginator {
    page: usize,
    num_items: usize,
    num_pages: usize,
}

pub struct CreateLensPage {
    pub lens_identifier: String,
    pub lens_data: Option<Lens>,

    pub lens_sources: Option<Vec<LensSource>>,
    pub lens_source_paginator: Option<LensSourcePaginator>,
    pub is_loading_lens_sources: bool,

    pub auth_status: AuthStatus,
    pub add_url_error: Option<String>,
    pub processing_action: Option<Action>,
    pub _context_listener: ContextHandle<AuthStatus>,
    pub _query_debounce: Option<JsValue>,
    pub _name_input_ref: NodeRef,
    pub _url_input_ref: NodeRef,
}

#[derive(Properties, PartialEq)]
pub struct CreateLensProps {
    pub lens: String,
    #[prop_or_default]
    pub onupdate: Callback<()>,
}

pub enum Action {
    AddSingleUrl,
    AddAllUrls,
}

pub enum Msg {
    AddUrl { include_all: bool },
    AddUrlError(String),
    ClearUrlError,
    Processing(Option<Action>),
    DeleteLensSource(LensSource),
    FilePicked { token: String, url: String },
    Reload,
    ReloadSources(usize),
    Save { display_name: String },
    SetLensData(Lens),
    SetLensSources(GetLensSourceResponse),
    OpenCloudFilePicker,
    UpdateContext(AuthStatus),
    UpdateDisplayName,
}

impl Component for CreateLensPage {
    type Message = Msg;
    type Properties = CreateLensProps;

    fn create(ctx: &Context<Self>) -> Self {
        // initialize gapi
        init_gapi(dotenv!("GOOGLE_CLIENT_ID"), dotenv!("GOOGLE_API_KEY"));

        let (auth_status, context_listener) = ctx
            .link()
            .context(ctx.link().callback(Msg::UpdateContext))
            .expect("No Message Context Provided");

        ctx.link()
            .send_message_batch(vec![Msg::Reload, Msg::ReloadSources(0)]);

        Self {
            lens_identifier: ctx.props().lens.clone(),
            lens_data: None,
            lens_sources: None,
            lens_source_paginator: None,
            is_loading_lens_sources: false,
            auth_status,
            add_url_error: None,
            processing_action: None,
            _context_listener: context_listener,
            _query_debounce: None,
            _name_input_ref: NodeRef::default(),
            _url_input_ref: NodeRef::default(),
        }
    }

    fn changed(&mut self, ctx: &Context<Self>, _old_props: &Self::Properties) -> bool {
        let new_lens = ctx.props().lens.clone();
        if self.lens_identifier != new_lens {
            self.lens_identifier = new_lens;

            let page = self
                .lens_source_paginator
                .as_ref()
                .map(|x| x.page)
                .unwrap_or(0);

            ctx.link()
                .send_message_batch(vec![Msg::Reload, Msg::ReloadSources(page)]);
            true
        } else {
            false
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        let link = ctx.link();
        match msg {
            Msg::AddUrl { include_all } => {
                if let Some(node) = self._url_input_ref.cast::<HtmlInputElement>() {
                    let url = node.value();

                    if let Err(_err) = url::Url::parse(&url) {
                        link.send_message(Msg::AddUrlError("Invalid Url".to_string()));
                    } else {
                        let new_source = LensAddDocument {
                            url,
                            doc_type: LensAddDocType::WebUrl {
                                include_all_suburls: include_all,
                            },
                        };
                        // Add to lens
                        let auth_status = self.auth_status.clone();
                        let identifier = self.lens_identifier.clone();
                        let link = link.clone();

                        if include_all {
                            spawn_local(async move {
                                link.send_message(Msg::Processing(Some(Action::AddAllUrls)));
                                let api = auth_status.get_client();
                                match api.validate_lens_source(&identifier, &new_source).await {
                                    Ok(response) => {
                                        if response.is_valid {
                                            node.set_value("");
                                            add_lens_source(&api, &new_source, &identifier, link)
                                                .await;
                                        } else if let Some(error_msg) = response.validation_msg {
                                            link.send_message_batch(vec![
                                                Msg::Processing(None),
                                                Msg::AddUrlError(error_msg),
                                            ])
                                        } else {
                                            link.send_message_batch(vec![
                                                Msg::Processing(None),
                                                Msg::AddUrlError(
                                                    "Unknown error adding url".to_string(),
                                                ),
                                            ])
                                        }
                                    }
                                    Err(error) => {
                                        log::error!("Unknown error adding url {:?}", error);
                                        link.send_message_batch(vec![
                                            Msg::Processing(None),
                                            Msg::AddUrlError(
                                                "Unknown error adding url".to_string(),
                                            ),
                                        ])
                                    }
                                }
                            })
                        } else {
                            node.set_value("");
                            spawn_local(async move {
                                link.send_message(Msg::Processing(Some(Action::AddSingleUrl)));
                                let api = auth_status.get_client();
                                add_lens_source(&api, &new_source, &identifier, link).await;
                            });
                        }
                    }
                }
                true
            }
            Msg::AddUrlError(msg) => {
                self.add_url_error = Some(msg);
                true
            }
            Msg::ClearUrlError => {
                self.add_url_error = None;
                true
            }
            Msg::Processing(action) => {
                self.processing_action = action;
                true
            }
            Msg::DeleteLensSource(source) => {
                // Add to lens
                let auth_status = self.auth_status.clone();
                let identifier = self.lens_identifier.clone();
                let link = link.clone();
                let page = self
                    .lens_source_paginator
                    .as_ref()
                    .map(|x| x.page)
                    .unwrap_or(0);
                spawn_local(async move {
                    link.send_message(Msg::Processing(Some(Action::AddAllUrls)));
                    let api = auth_status.get_client();
                    if let Err(error) = api.delete_lens_source(&identifier, &source.doc_uuid).await
                    {
                        log::error!("error deleting lens source: {error}");
                    } else {
                        // Reload data if successful
                        link.send_message(Msg::ReloadSources(page));
                    }
                });
                false
            }
            Msg::FilePicked { token, url } => {
                let new_source = LensAddDocument {
                    url,
                    doc_type: LensAddDocType::GDrive { token },
                };

                // Add to lens
                let auth_status = self.auth_status.clone();
                let identifier = self.lens_identifier.clone();
                let link = link.clone();
                spawn_local(async move {
                    let api = auth_status.get_client();
                    if let Err(err) = api.lens_add_source(&identifier, &new_source).await {
                        log::error!("error adding gdrive source: {err}");
                    } else {
                        // Reload data if successful
                        link.send_message(Msg::Reload);
                    }
                });
                true
            }
            Msg::Reload => {
                let auth_status = self.auth_status.clone();
                let identifier = self.lens_identifier.clone();
                let link = link.clone();
                spawn_local(async move {
                    let api = auth_status.get_client();
                    match api.lens_retrieve(&identifier).await {
                        Ok(lens) => link.send_message(Msg::SetLensData(lens)),
                        Err(ApiError::ClientError(msg)) => {
                            // Unauthorized
                            if msg.code == 400 {
                                let navi = link.navigator().expect("No navigator");
                                navi.push(&crate::Route::Start);
                            }

                            log::error!("error retrieving lens: {msg}");
                        }
                        Err(err) => log::error!("error retrieving lens: {err}"),
                    }
                });

                false
            }
            Msg::ReloadSources(page) => {
                let auth_status = self.auth_status.clone();
                let identifier = self.lens_identifier.clone();
                let link = link.clone();
                self.is_loading_lens_sources = true;
                spawn_local(async move {
                    let api: crate::client::ApiClient = auth_status.get_client();
                    match api.lens_retrieve_sources(&identifier, page).await {
                        Ok(lens) => link.send_message(Msg::SetLensSources(lens)),
                        Err(ApiError::ClientError(msg)) => {
                            // Unauthorized
                            if msg.code == 400 {
                                let navi = link.navigator().expect("No navigator");
                                navi.push(&crate::Route::Start);
                            }

                            log::error!("error retrieving lens: {msg}");
                        }
                        Err(err) => log::error!("error retrieving lens: {err}"),
                    }
                });

                true
            }
            Msg::Save { display_name } => {
                let auth_status = self.auth_status.clone();
                let identifier = self.lens_identifier.clone();
                let link = link.clone();
                let onupdate_callback = ctx.props().onupdate.clone();
                spawn_local(async move {
                    let api = auth_status.get_client();
                    if api.lens_update(&identifier, &display_name).await.is_ok() {
                        link.send_message(Msg::Reload);
                        onupdate_callback.emit(());
                    }
                });
                false
            }
            Msg::SetLensData(lens_data) => {
                self.lens_data = Some(lens_data);
                true
            }
            Msg::SetLensSources(sources) => {
                self.is_loading_lens_sources = false;
                self.lens_source_paginator = Some(LensSourcePaginator {
                    page: sources.page,
                    num_items: sources.num_items,
                    num_pages: sources.num_pages,
                });

                self.lens_sources = Some(sources.results);
                true
            }
            Msg::OpenCloudFilePicker => {
                let link = link.clone();
                spawn_local(async move {
                    let cb = Closure::wrap(Box::new(move |token: JsValue, payload: JsValue| {
                        if let (Ok(token), Ok(url)) = (
                            serde_wasm_bindgen::from_value::<String>(token),
                            serde_wasm_bindgen::from_value::<String>(payload),
                        ) {
                            link.send_message(Msg::FilePicked { token, url });
                        }
                    }) as Box<dyn Fn(JsValue, JsValue)>);

                    if let Err(err) = create_picker(&cb).await {
                        log::error!("create_picker error: {:?}", err);
                    }
                    cb.forget();
                });
                false
            }
            Msg::UpdateContext(auth_status) => {
                self.auth_status = auth_status;
                let page = self
                    .lens_source_paginator
                    .as_ref()
                    .map(|x| x.page)
                    .unwrap_or(0);
                link.send_message_batch(vec![Msg::Reload, Msg::ReloadSources(page)]);
                true
            }
            Msg::UpdateDisplayName => {
                if let Some(timeout_id) = &self._query_debounce {
                    clear_timeout(timeout_id.clone());
                    self._query_debounce = None;
                }

                {
                    if let Some(node) = self._name_input_ref.cast::<HtmlInputElement>() {
                        let display_name = node.value();
                        let link = link.clone();
                        let handle = Timeout::new(QUERY_DEBOUNCE_MS, move || {
                            link.send_message(Msg::Save { display_name })
                        });

                        let id = handle.forget();
                        self._query_debounce = Some(id);
                    }
                }

                false
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let link = ctx.link();

        let sources = self.lens_sources.as_ref().cloned().unwrap_or_default();

        let delete_callback = {
            let link = link.clone();
            Callback::from(move |lens_source| link.send_message(Msg::DeleteLensSource(lens_source)))
        };
        let source_html = sources
            .iter()
            .map(|x| html! { <LensSourceComponent delete_callback={delete_callback.clone()} source={x.clone()} /> })
            .collect::<Html>();

        let add_url_actions = if let Some(action) = &self.processing_action {
            match action {
                Action::AddAllUrls => {
                    html! {
                        <>
                        <Btn disabled=true>{"Add data from URL"}</Btn>
                        <Btn disabled=true>
                          <icons::RefreshIcon
                            classes="mr-1"
                            width="w-3"
                            height="h-3"
                            animate_spin=true/>
                            {"Add all URLs from Site"}
                        </Btn>
                        </>
                    }
                }
                Action::AddSingleUrl => {
                    html! {
                        <>
                        <Btn disabled=true>
                          <icons::RefreshIcon
                            classes="mr-1"
                            width="w-3"
                            height="h-3"
                            animate_spin=true/>
                          {"Add data from URL"}
                        </Btn>
                        <Btn disabled=true>{"Add all URLs from Site"}</Btn>
                        </>
                    }
                }
            }
        } else {
            html! {
                <>
                <Btn onclick={link.callback(|_| Msg::AddUrl {include_all: false})}>{"Add data from URL"}</Btn>
                <Btn onclick={link.callback(|_| Msg::AddUrl {include_all: true} )}>{"Add all URLs from Site"}</Btn>
                </>
            }
        };

        let is_loading_sources = self.is_loading_lens_sources;
        html! {
            <div>
                <div class="flex flex-row items-center px-8 pt-6">
                    <div>
                    {if let Some(lens_data) = self.lens_data.as_ref() {
                        html! {
                            <input
                                class="border-b-4 border-neutral-600 pt-3 pb-1 bg-neutral-800 text-white text-2xl outline-none active:outline-none focus:outline-none caret-white"
                                type="text"
                                spellcheck="false"
                                tabindex="-1"
                                value={lens_data.display_name.to_string()}
                                oninput={link.callback(|_| Msg::UpdateDisplayName)}
                                ref={self._name_input_ref.clone()}
                            />
                        }
                    } else {
                        html! {
                            <h2 class="bold text-xl ">{"Loading"}</h2>
                        }
                    }}
                    </div>
                </div>
                <div class="flex flex-col gap-8 px-8 py-4">
                    <div class="flex flex-col gap-4">
                        <div class="flex flex-row gap-4 items-center">
                            <input ref={self._url_input_ref.clone()}
                                type="text"
                                class="rounded p-2 text-sm text-neutral-800"
                                placeholder="https://example.com"
                            />
                            {add_url_actions}
                            <div class="text-sm text-red-700">{self.add_url_error.clone()}</div>
                        </div>
                        <div><Btn onclick={link.callback(|_| Msg::OpenCloudFilePicker)}>{"Add data from Google Drive"}</Btn></div>
                    </div>
                    {if let Some(paginator) = self.lens_source_paginator.clone() {
                        html! {
                            <div class="flex flex-col">
                                <div class="flex flex-row mb-2 text-sm font-semibold uppercase text-cyan-500">
                                    <div>{format!("Sources ({})", paginator.num_items)}</div>
                                    <div class="ml-auto">
                                        <Btn size={BtnSize::Sm} onclick={link.callback(move |_| Msg::ReloadSources(paginator.page))}>
                                            <icons::RefreshIcon
                                                classes="mr-1"
                                                width="w-3"
                                                height="h-3"
                                                animate_spin={is_loading_sources}
                                            />
                                            {"Refresh"}
                                        </Btn>
                                    </div>
                                </div>
                                <div class="flex flex-col">{source_html}</div>
                                {if paginator.num_pages > 1 {
                                    html! {
                                        <div>
                                            <Paginator
                                                disabled={is_loading_sources}
                                                cur_page={paginator.page}
                                                num_pages={paginator.num_pages}
                                                on_select_page={link.callback(Msg::ReloadSources)}
                                            />
                                        </div>
                                    }
                                } else {
                                    html! {}
                                }}
                            </div>
                        }
                    } else { html! {} }}
                </div>
            </div>
        }
    }
}

#[derive(Properties, PartialEq)]
struct LensSourceComponentProps {
    source: LensSource,
    delete_callback: Callback<LensSource>,
}

#[function_component(LensSourceComponent)]
fn lens_source_comp(props: &LensSourceComponentProps) -> Html {
    let source = props.source.clone();
    let callback = props.delete_callback.clone();

    let doc_type_icon = match source.doc_type {
        LensDocType::Audio => html! {
            <icons::FileExtIcon ext={"mp3"} class="h-4 w-4" />
        },
        LensDocType::GDrive => html! { <icons::GDrive /> },
        LensDocType::Web => html! {
            <div class="flex flex-col items-center">
                <icons::GlobeIcon width="w-4" height="h-4" />
                <div class="text-xs">{"Web"}</div>
            </div>
        },
    };

    let status_icon = match source.status.as_ref() {
        "Deployed" => html! { <icons::BadgeCheckIcon classes="fill-green-500" /> },
        _ => html! { <icons::RefreshIcon animate_spin={true} /> },
    };

    let delete = {
        let source = source.clone();
        Callback::from(move |_e: MouseEvent| callback.emit(source.clone()))
    };

    html! {
        <div class="py-4 flex flex-row items-center gap-2">
            <div class="flex-none px-2">
                {doc_type_icon}
            </div>
            <div class="overflow-hidden">
                <div class="text-sm">
                    <a href={source.url.clone()} target="_blank" class="text-cyan-500 underline">
                        {source.display_name.clone()}
                    </a>
                </div>
                <div class="text-sm ml-1 text-neutral-600">{source.url.clone()}</div>
            </div>
            <div class="flex px-2 space-x-2 flex-row items-center text-base ml-auto">
                {status_icon}
                <Btn size={BtnSize::Xs} onclick={delete}>
                  <icons::TrashIcon classes={classes!("text-neutral-400")} height="h-4" width="h-4" />
                </Btn>
            </div>

        </div>
    }
}

async fn add_lens_source(
    api: &ApiClient,
    new_source: &LensAddDocument,
    identifier: &str,
    link: Scope<CreateLensPage>,
) {
    if let Err(err) = api.lens_add_source(identifier, new_source).await {
        log::error!("error adding url source: {err}");
        match err {
            ApiError::ClientError(msg) => {
                link.send_message_batch(vec![Msg::Processing(None), Msg::AddUrlError(msg.message)])
            }
            _ => link.send_message_batch(vec![
                Msg::Processing(None),
                Msg::AddUrlError(err.to_string()),
            ]),
        };
    } else {
        link.send_message(Msg::ClearUrlError);
        // Reload data if successful
        link.send_message_batch(vec![
            Msg::Processing(None),
            Msg::ClearUrlError,
            Msg::ReloadSources(0),
        ]);
    }
}