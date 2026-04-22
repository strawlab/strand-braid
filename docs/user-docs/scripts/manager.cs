//MIT License

//Copyright (c) 2021 Renaud Bastien

//Permission is hereby granted, free of charge, to any person obtaining a copy
//of this software and associated documentation files (the "Software"), to deal
//in the Software without restriction, including without limitation the rights
//to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//copies of the Software, and to permit persons to whom the Software is
//furnished to do so, subject to the following conditions:

//The above copyright notice and this permission notice shall be included in all
//copies or substantial portions of the Software.

//THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
//SOFTWARE.



// Read the stream from Braid with unity
// This script reads the stream coming from url:port and move the GameObject sphere accordingly
// The script should be attached to any GameObject in the scene. the port and url
// are public objects and can be changed directly in the editor
// The moving object sphere should be defined in the editor and linked to this script



using System;
using System.Collections;
using System.Collections.Generic;
using System.Linq;
using System.IO;
using System.Threading;
using UnityEngine;
using System.Threading.Tasks;
using System.Net;
using System.Net.Sockets;
using static StreamStructure;

public class manager : MonoBehaviour
{

    private Socket sock;
    private HttpWebRequest request2;
    public string url = "192.168.0.131";
    public int port = 9992;
    public bool running = true;
    private StreamReader streamer;
    private Thread threadReader;
    private string DATA_PREFIX = "data: ";
    public GameObject sphere;
    private Vector3 positionUpdate = new Vector3();


    public void StreamReader()
    {
        Debug.Log("started");
        while (running)
        {
            string line = streamer.ReadLine();


            if (line == "event: braid")
            {
                line = streamer.ReadLine().Substring(5);
                var ss =  StreamStructure.CreateFromJSON(line);
                try
                {
                    if (ss.msg.Update.x !=0)
                    positionUpdate = new Vector3(ss.msg.Update.x, ss.msg.Update.z, ss.msg.Update.y);
                }
                catch
                {
                }

            }
            
        }
        Debug.Log("stopped");
    }
    void Start()
    {


        string uri = "http://" + url + ":" + port.ToString();
        Debug.Log(uri);
        var request = (HttpWebRequest)WebRequest.Create(uri);
        request.KeepAlive = true;
        request.CookieContainer = new CookieContainer();
        var response = (HttpWebResponse)request.GetResponse();
        Debug.Log(response.StatusCode);
        string uriEvents = uri+"/events";
        Debug.Log(uriEvents);
        request2 = (HttpWebRequest)WebRequest.Create(uriEvents);
        request2.KeepAlive = true;
        request2.CookieContainer = request.CookieContainer;
        request2.Accept = "text/event-stream";
        HttpWebResponse res = (HttpWebResponse)request2.GetResponse();
        streamer = new StreamReader(res.GetResponseStream(), System.Text.Encoding.Default);
        threadReader = new Thread(new ThreadStart(StreamReader));
        threadReader.Start();


    }  


    void Update() 
    {
        sphere.transform.position = positionUpdate;
    }
}
