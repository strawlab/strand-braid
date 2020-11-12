port module Main exposing (..)

import Http

import Html exposing (..)
import Html.Attributes exposing (type_, href, class, style)
import Json.Decode
import Json.Decode as Decode
import Json.Decode exposing (int, string, float, bool, nullable, Decoder, list, field)
import Json.Encode as Encode

import Material
import Material.Options as Options exposing (css)
import Material.Layout as Layout
import Material.Slider as Slider
import Material.Button as Button
import Material.Grid exposing (grid, offset, cell, size, Device(..))

type alias ServerState =
  { image_names: (List String)
  }

decodeServerState : Decoder ServerState
decodeServerState =
  Json.Decode.map ServerState
    (field "image_names" (Json.Decode.list Json.Decode.string) )

type ReadyState
  = Connecting
  | Open
  | Closed

type alias Model =
    { server_state : ServerState
    , fail_msg : String
    , framerate10 : Float
    , mdl : Material.Model
    , ready_state : ReadyState
    , is_live_preview : Bool
    }

init : Bool -> ( Model, Cmd Msg )
init is_live_preview =
    ( { server_state = {
            image_names = []
          }
      , fail_msg = ""
      , framerate10 = 100.0
      , mdl = Material.model
      , ready_state = Connecting
      , is_live_preview = is_live_preview
      }
    , Cmd.none )

type Msg
  = NewServerState ServerState
  | SliderChange Float
  | Mdl (Material.Msg Msg)
  | FailedDecode String
  | NewReadyState ReadyState
  | CallbackDone (Result Http.Error String)
  | BadUserInput
  | ShowLiveView String

update : Msg -> Model -> (Model, Cmd Msg)
update msg model =
  case msg of
    NewServerState new_ss ->
        ({model | server_state = new_ss}, Cmd.none)

    SliderChange value -> ({model | framerate10=value}, do_slider_change value)

    Mdl msg_ ->
        Material.update Mdl msg_ model

    FailedDecode str -> ({model | fail_msg = str}, Cmd.none)

    NewReadyState rs ->
        ({model | ready_state = rs}, Cmd.none)

    CallbackDone result ->
        (model, Cmd.none)

    BadUserInput -> (model, Cmd.none)

    ShowLiveView name -> (model, show_live_view name)

type alias Mdl =
    Material.Model

viewName : Model -> Int -> String -> Html Msg
viewName model idx name =
    li [] [
        Button.render Mdl [idx] model.mdl
        [ Button.raised
        , Options.onClick (ShowLiveView name)
        ]
        [ text ("Show " ++ name) ]
    ]

viewControls : Model -> Html Msg
viewControls model =
    Layout.render Mdl model.mdl
    [ Layout.fixedHeader
    ]
    { header = []
    , drawer = []
    , tabs = ([], [])
    , main = [
      div [] [
        text model.fail_msg
      ]
      , div [ (Html.Attributes.style [("padding", "10px")]) ]
            [
              h3 [] [text "rt-image-viewer"]
            , ul [] (List.indexedMap (viewName model) model.server_state.image_names)
            ]
      ]}

viewLivePreview : Model -> Html Msg
viewLivePreview model =
    Layout.render Mdl model.mdl
    [ Layout.fixedHeader
    ]
    { header = []
    , drawer = []
    , tabs = ([], [])
    , main = [
        div [] [
            text model.fail_msg
        ]
        , div [ (Html.Attributes.style [("padding", "2px")]) ] [
            grid []
                [
                cell [ size All 6 ] [
                div [] [
                    div [] [
                    if model.framerate10 < 100 then
                        (text ("update rate: " ++ (toString (model.framerate10/10.0)) ++ " frames per second"))
                        else
                        (text "update rate: maximum")
                    ]
                    , Slider.view
                    [ Slider.onChange SliderChange
                    , Slider.value model.framerate10
                    , Slider.max 100
                    , Slider.min 1
                    ]
                ]
                ]
            ]
            , div [] [
                span [ Html.Attributes.attribute "id" "firehose-text",
                            Html.Attributes.style [ ("display", "block")
                                                , ("width", "100%")
                                                , ("text-align", "right") ]
                        ] [
                ]
            ]
            , div [] [
                img [ Html.Attributes.attribute "id" "firehose-img", class "live-preview" ] []
            ]
        ]
      ]}

view : Model -> Html Msg
view model =
    case model.is_live_preview of
        True -> viewLivePreview model
        False -> viewControls model

type alias ToServerMsg =
    { callback_name : String
    , callback_args : List String
    }

do_slider_change : Float -> Cmd Msg
do_slider_change value =
  set_max_framerate (value/10.0)

callbackEncoded : String -> Encode.Value -> Encode.Value
callbackEncoded name args =
    let
        list =
            [ ( "name", Encode.string name )
            , ( "args",  args )
            ]
    in
        list
            |> Encode.object

postCallback : Http.Body -> Http.Request String
postCallback body =
  Http.post "callback" body string

getServerStateOrFail : String -> Msg
getServerStateOrFail encoded =
  case Json.Decode.decodeString decodeServerState encoded of
    Ok (ssc) -> NewServerState ssc
    Err msg -> FailedDecode msg

port show_live_view : String -> Cmd msg
port set_max_framerate : Float -> Cmd msg

port event_source_data : (String -> msg) -> Sub msg
port ready_state : (Int -> msg) -> Sub msg
port new_image_name : (String -> msg) -> Sub msg

decodeReadyState : Int -> Msg
decodeReadyState code =
  case to_ready_state code of
    Ok(rs) -> NewReadyState rs
    Err msg -> FailedDecode msg

to_ready_state : Int -> Result String ReadyState
to_ready_state code =
  case code of
    0 -> Ok(Connecting)
    1 -> Ok(Open)
    2 -> Ok(Closed)
    _ -> Err("unknown ReadyState code")

subscriptions : Model -> Sub Msg
subscriptions model =
  Sub.batch
      [ Layout.subs Mdl model.mdl
      , event_source_data getServerStateOrFail
      , ready_state decodeReadyState
      ]

-- main : Program Never Model Msg
main : Program Bool Model Msg
main =
    programWithFlags { view = view, init = init, update = update, subscriptions = subscriptions }
